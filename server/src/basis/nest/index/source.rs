//! 来源 ↔ 关系 的完整弱关联：这份来源的窗口被哪些关系压着。
//!
//! 与关系索引分工不同：索引按 VA 找关系、只服务本地址空间；这份关联按**来源**
//! 找关系、跨地址空间，专门给“把这些来源页撤下来”的枚举用；它只负责发现，
//! 不是第二份权威。条目是**一条一次分配**的 Box 链：关系退场就把自己那一条摘掉释放，
//! 所以没有“全局 Vec 容量高水位”这种账。
//! 自身锁内不取关系锁、地址空间锁，也不升级任何临时强引用（死项的 strong_count 仅为清死索引）。

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::sync::Weak;

use crate::metal::sync::LegacyLock;
use crate::metal::sync::OnceLock;

use super::super::Span;
use crate::basis::flat::FlatRange;
use crate::basis::nest::{NestError, authority::RangeBinding};

// 关联里的一条：这段来源窗口被这个关系压着。
//
// 一条一次分配、随关系退场就释放：整张关联没有全局容量高水位。

struct Record {
    source_id: kernel_api::model::flat::FlatId,
    reservation: Weak<Span>,
    binding: Option<Weak<RangeBinding>>,
    next: Option<alloc::boxed::Box<Record>>,
}

/// 全核唯一的一份关联：单链头，每条一个关系。
struct SourceReservations {
    head: LegacyLock<crate::metal::sync::order::SourceIndex, Option<Box<Record>>>,
}

fn registry() -> &'static SourceReservations {
    static REGISTRY: OnceLock<SourceReservations> = OnceLock::new();
    REGISTRY.get_or_init(|| SourceReservations {
        head: LegacyLock::new(None),
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
        if node.reservation.strong_count() == 0
            || node
                .binding
                .as_ref()
                .is_some_and(|binding| binding.strong_count() == 0)
        {
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

/// 关系退场：它那一条从链上摘下并释放。在 `Span::drop` 里调用：
/// 那时它的强计数已经归零，重建就会把这一条摘掉。
pub(crate) fn forget() {
    let mut head = registry().head.lock();
    rebuild(&mut head, |_| {});
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
    // 先备好节点：锁内不分配也不升级任何强引用。
    let record = Box::try_new(Record {
        source_id,
        reservation: Arc::downgrade(reservation),
        binding: binding.map(Arc::downgrade),
        next: None,
    })
    .map_err(|_| NestError::OutOfMemory)?;
    let identity = Arc::as_ptr(reservation);
    let mut head = registry().head.lock();
    let mut duplicate = false;
    rebuild(&mut head, |known| {
        if known.source_id == source_id
            && known.reservation.as_ptr() == identity
            && match (&known.binding, binding) {
                (None, None) => true,
                (Some(known), Some(binding)) => known.as_ptr() == Arc::as_ptr(binding),
                _ => false,
            }
        {
            duplicate = true;
        }
    });
    if !duplicate {
        let mut record = record;
        record.next = head.take();
        *head = Some(record);
    }
    Ok(())
}

/// Allocation-free wake after Page admission reopens. Pointer order is used only
/// for this bounded traversal, never as identity or authority. Removed mappings
/// have no live waiter; newly added waiters recheck the already-open predicate.
pub(crate) fn wake_all() {
    forget();
    let mut cursor = 0;
    loop {
        let candidate = {
            let head = registry().head.lock();
            let mut node = head.as_deref();
            let mut next: Option<(usize, Weak<Span>)> = None;
            while let Some(record) = node {
                let key = record.reservation.as_ptr() as usize;
                if key > cursor && next.as_ref().is_none_or(|(known, _)| key < *known) {
                    next = Some((key, record.reservation.clone()));
                }
                node = record.next.as_deref();
            }
            next
        };
        let Some((key, candidate)) = candidate else {
            break;
        };
        cursor = key;
        let route = candidate
            .upgrade()
            .and_then(|reservation| reservation.wake_key());
        if let Some((space, reservation)) = route {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::Reservation {
                space,
                reservation: reservation,
            });
        }
    }
}
