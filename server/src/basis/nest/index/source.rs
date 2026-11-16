//! 来源 ↔ 关系 的完整弱关联：这份来源的窗口被哪些关系压着。
//!
//! 与关系索引分工不同：索引按 VA 找关系、只服务本地址空间；这份关联按**来源**
//! 找关系、跨地址空间，专门给“把这些来源页撤下来”的枚举用；它只负责发现，
//! 不是第二份权威。条目是**一条一次分配**的 Box 链：关系退场请求有界清理自己的弱条目，
//! 所以没有“全局 Vec 容量高水位”这种账。
//! 自身锁内不取关系锁、地址空间锁，也不升级任何临时强引用（死项的 strong_count 仅为清死索引）。

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::sync::Weak;

use crate::basis::sync::LegacyLock;
use crate::metal::sync::OnceLock;

use super::super::Span;
use crate::basis::flat::FlatRange;
use crate::basis::nest::{NestError, NestId, authority::RangeBinding};

// 关联里的一条：这段来源窗口被这个关系压着。
//
// 一条一次分配、随关系退场就释放：整张关联没有全局容量高水位。

struct Record {
    source_id: kernel_api::model::flat::FlatId,
    span: Weak<Span>,
    binding: Option<Weak<RangeBinding>>,
    wake: (NestId, usize),
    next: Option<alloc::boxed::Box<Record>>,
}

/// 全核唯一的一份关联：单链头，每条一个关系。
struct SourceSpans {
    records: LegacyLock<crate::basis::sync::order::SourceIndex, Records>,
}

/// One discovery list, partitioned by the current notification pass. Registering
/// during a pass cannot continually prepend work ahead of its remaining nodes.
/// Neither list owns publication or retirement authority.
struct Records {
    pending: Option<Box<Record>>,
    visited: Option<Box<Record>>,
    pass: Pass,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Idle,
    Current {
        notice: Notice,
        next: Option<Notice>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Notice {
    Prune,
    Wake,
}
impl Pass {
    const fn request(self, requested: Notice) -> Self {
        match self {
            Self::Idle => Self::Current {
                notice: requested,
                next: None,
            },
            Self::Current { notice, next } => Self::Current {
                notice,
                next: Some(match (next, requested) {
                    (Some(Notice::Wake), _) | (_, Notice::Wake) => Notice::Wake,
                    _ => Notice::Prune,
                }),
            },
        }
    }
    const fn finish(self) -> Self {
        match self {
            Self::Current {
                next: Some(notice), ..
            } => Self::Current { notice, next: None },
            _ => Self::Idle,
        }
    }
}

enum Step {
    Idle,
    Visited(Option<(NestId, usize)>),
}
impl Records {
    const fn new() -> Self {
        Self {
            pending: None,
            visited: None,
            pass: Pass::Idle,
        }
    }
    fn step(&mut self) -> Step {
        if self.pass == Pass::Idle {
            return Step::Idle;
        }
        let candidate = if let Some(mut record) = self.pending.take() {
            self.pending = record.next.take();
            if record.live() {
                let candidate = matches!(
                    self.pass,
                    Pass::Current {
                        notice: Notice::Wake,
                        ..
                    }
                )
                .then_some(record.wake);
                record.next = self.visited.take();
                self.visited = Some(record);
                candidate
            } else {
                None
            }
        } else {
            None
        };
        if self.pending.is_none() {
            self.pending = self.visited.take();
            self.pass = self.pass.finish();
        }
        Step::Visited(candidate)
    }
}
impl Record {
    fn live(&self) -> bool {
        self.span.strong_count() != 0
            && self
                .binding
                .as_ref()
                .is_none_or(|binding| binding.strong_count() != 0)
    }
}

fn registry() -> &'static SourceSpans {
    static REGISTRY: OnceLock<SourceSpans> = OnceLock::new();
    REGISTRY.get_or_init(|| SourceSpans {
        records: LegacyLock::new(Records::new()),
    })
}

/// 把链走一遍：死项当场退场（清它只靠弱计数，不升级），活项重建；
/// `keep` 对每个活项调用一次（不取锁、不分配）。
///
/// 重建保持相对次序：一次查询不该顺手动别人登记的先后。翻转只用指针，不分配。
fn rebuild(head: &mut Option<Box<Record>>, mut keep: impl FnMut(&Record)) {
    let mut flipped: Option<Box<Record>> = None;
    let mut cursor = head.take();
    while let Some(mut node) = cursor {
        cursor = node.next.take();
        if !node.live() {
            continue;
        }
        keep(&node);
        node.next = flipped.take();
        flipped = Some(node);
    }
    *head = reverse(flipped);
}

/// 原地翻转一条链（正向拼回）——只用指针，不分配。
fn reverse(mut chain: Option<Box<Record>>) -> Option<Box<Record>> {
    let mut out: Option<Box<Record>> = None;
    while let Some(mut node) = chain {
        chain = node.next.take();
        node.next = out.take();
        out = Some(node);
    }
    out
}

/// Span destruction retains a pruning obligation instead of recursively
/// scanning the whole registry from a last temporary strong-reference drop.
/// Pruning does not wake unrelated waiters or infer any retirement completion.
pub(crate) fn forget() {
    let mut records = registry().records.lock();
    records.pass = records.pass.request(Notice::Prune);
}

/// 登记一段来源窗口与一个关系的关联：同一关系在同一份来源上只留一条。
///
/// 关系的首条 PTE 之前调用；失败即整次安装失败，不留半条。
/// 键按**强持的** `FlatRange` 取（调用方持有它），不缓存可回收的裸身份。
pub(crate) fn register(
    source: &FlatRange,
    reservation: &Arc<Span>,
    binding: Option<&Arc<RangeBinding>>,
) -> Result<(), NestError> {
    let source_id = source.source_id();
    // Registration still holds the actual mapping/Nest owners. Preserve only
    // immutable software identities for later notification, never a permission
    // or retirement snapshot. A scan must not become a final owner/destructor.
    let wake = reservation.wake_key().ok_or(NestError::NoRegion)?;
    // 先备好节点：锁内不分配也不升级任何强引用。
    let record = Box::try_new(Record {
        source_id,
        span: Arc::downgrade(reservation),
        binding: binding.map(Arc::downgrade),
        wake,
        next: None,
    })
    .map_err(|_| NestError::OutOfMemory)?;
    let identity = Arc::as_ptr(reservation);
    let mut records = registry().records.lock();
    let mut duplicate = false;
    let mut inspect = |known: &Record| {
        if known.source_id == source_id
            && known.span.as_ptr() == identity
            && match (&known.binding, binding) {
                (None, None) => true,
                (Some(known), Some(binding)) => known.as_ptr() == Arc::as_ptr(binding),
                _ => false,
            }
        {
            duplicate = true;
        }
    };
    rebuild(&mut records.pending, &mut inspect);
    rebuild(&mut records.visited, &mut inspect);
    if !duplicate {
        let mut record = record;
        // Admission waiters register against the current open predicate. A new
        // record need not delay an older completion pass; any later completion
        // requests a further pass that includes it.
        let head = if records.pass == Pass::Idle {
            &mut records.pending
        } else {
            &mut records.visited
        };
        record.next = head.take();
        *head = Some(record);
    }
    Ok(())
}

/// Coalesce a completion without losing events that arrive after some nodes
/// have already been visited. The next pass rechecks those earlier nodes too.
pub(crate) fn request() {
    let mut records = registry().records.lock();
    records.pass = records.pass.request(Notice::Wake);
}

pub(crate) fn pending() -> bool {
    registry().records.lock().pass != Pass::Idle
}

/// Inspect at most four discovery records, including expired ones. No whole-list
/// search, pointer ordering, allocation or cursor reconstruction occurs here.
/// Caller excludes Task cancellation across this synchronous, infallible turn;
/// every claimed candidate is signalled before that exclusion ends. Sign already
/// limits a queue to 64 listeners and forwarding to eight destinations. The pass
/// upgrades no Span/Nest and cannot trigger their final destructors. Sign effects
/// run outside the index lock; this is not a hard bound on lock/firmware latency.
pub(crate) fn advance() -> bool {
    for _ in 0..4 {
        let candidate = match registry().records.lock().step() {
            Step::Idle => break,
            Step::Visited(candidate) => candidate,
        };
        if let Some((space, span)) = candidate {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::Span { space, span });
        }
    }
    pending()
}
