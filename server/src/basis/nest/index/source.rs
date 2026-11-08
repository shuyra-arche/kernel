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
use alloc::vec::Vec;

use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;

use kernel_api::address::PageIndex;
use kernel_api::address::VirtAddr;

use super::super::Span;
use crate::basis::flat::FlatRange;
use crate::basis::nest::NestError;

// 关联里的一条：这段来源窗口被这个关系压着。
//
// 一条一次分配、随关系退场就释放：整张关联没有全局容量高水位。

struct Record {
    source_id: usize,
    mapping: Weak<Span>,
    next: Option<alloc::boxed::Box<Record>>,
}

/// 全核唯一的一份关联：单链头，每条一个关系。
struct SourceMappings {
    head: SpinLock<Option<Box<Record>>>,
}

fn registry() -> &'static SourceMappings {
    static REGISTRY: OnceLock<SourceMappings> = OnceLock::new();
    REGISTRY.get_or_init(|| SourceMappings {
        head: SpinLock::new_level(Level::L3, None),
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
        if node.mapping.strong_count() == 0 {
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
pub(crate) fn register(source: &FlatRange, mapping: &Arc<Span>) -> Result<(), NestError> {
    let source_id = source.source_id();
    // 先备好节点：锁内不分配也不升级任何强引用。
    let record = Box::try_new(Record {
        source_id,
        mapping: Arc::downgrade(mapping),
        next: None,
    })
    .map_err(|_| NestError::OutOfMemory)?;
    let identity = Arc::as_ptr(mapping);
    let mut head = registry().head.lock();
    let mut duplicate = false;
    rebuild(&mut head, |known| {
        if known.source_id == source_id && known.mapping.as_ptr() == identity {
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

/// 两端候选关系的一份弱快照：两份来源窗口上登记过的全部关系。
///
/// 与 `mappings_over` / `pressings_over` 的强快照不同，这里**只收 `Weak`**：它要在
/// `Flat` 锁内调用，不能在那里升级强引用（`Flat → Span` 是反向锁阶），也拿不到
/// 关系自己的锁。于是它交出的是一份**完整上界**——谁真的压着本次两端的那一页，
/// 留给占槽后的 `affected_over` 按关系自己的不可变窗口筛。
///
/// 两份来源键各收一次，两端同 Flat 时同一份登记会被收两次；去重不在这里做
/// （占槽前的时间越短越好），留给 `affected_over` 在原地收回。
/// 先按数出的上界一次性备容量，再只 clone `Weak`：链上不留半条、也不在锁内分配多次。
pub(crate) fn candidates_over(
    source: &FlatRange,
    target: &FlatRange,
) -> Result<Vec<Weak<Span>>, NestError> {
    let source_id = source.source_id();
    let target_id = target.source_id();
    let mut head = registry().head.lock();
    // 第一遍：清死项、重建链，数出完整上界（两端各计一次）。
    let mut bound = 0usize;
    rebuild(&mut head, |known| {
        if known.source_id == source_id {
            bound += 1;
        }
        if known.source_id == target_id {
            bound += 1;
        }
    });
    // 链已经归位：之后的可失败步骤不会弄丢任何登记。
    let mut candidates = Vec::new();
    candidates
        .try_reserve(bound)
        .map_err(|_| NestError::OutOfMemory)?;
    let mut cursor = head.as_deref();
    while let Some(node) = cursor {
        if node.source_id == source_id {
            candidates.push(node.mapping.clone());
        }
        if node.source_id == target_id {
            candidates.push(node.mapping.clone());
        }
        cursor = node.next.as_deref();
    }
    Ok(candidates)
}

/// 从两端候选里收回这次移交真正动过的关系：压着源页或目标页的那些。
///
/// 在无 core 锁的区域里跑：按每个关系自己（不可变）的来源窗口过滤——窗口之外
/// 的候选是上界里多收的，不是这次动过的；两端同 Flat 时同一份登记会被收两次，
/// 按 `Weak` 身份去掉重复的那条。过滤与去重全在原地做，不分配。
pub(crate) fn affected_over(
    candidates: Vec<Weak<Span>>,
    source: &FlatRange,
    source_page: PageIndex,
    target: &FlatRange,
    target_page: PageIndex,
) -> Vec<Weak<Span>> {
    let mut affected = candidates;
    affected.retain(|candidate| {
        let Some(mapping) = candidate.upgrade() else {
            return false;
        };
        pressing_page(&mapping, source, source_page).is_some()
            || pressing_page(&mapping, target, target_page).is_some()
    });
    distinct(&mut affected);
    affected
}

/// 同一个关系在清单里只留一条；原地压缩，不分配。
fn distinct(mappings: &mut Vec<Weak<Span>>) {
    let mut kept = 0usize;
    let mut read = 0usize;
    while read < mappings.len() {
        let seen = (0..kept).any(|at| Weak::ptr_eq(&mappings[at], &mappings[read]));
        if seen {
            mappings.swap_remove(read);
        } else {
            kept += 1;
            read += 1;
        }
    }
}

/// 这段关系把来源 `source` 上的 `page` 放在哪个虚拟页上；不是它的窗口就没有。
fn pressing_page(mapping: &Span, source: &FlatRange, page: PageIndex) -> Option<VirtAddr> {
    let inner = mapping.state.lock();
    if inner.source.source_id() != source.source_id() {
        return None;
    }
    let offset = page.get().checked_sub(inner.source.index().get())?;
    if offset >= inner.source.page_count().get() {
        return None;
    }
    let address = mapping
        .initial_start()
        .as_usize()
        .checked_add(offset.checked_mul(kernel_api::address::PAGE_SIZE)?)?;
    Some(crate::metal::mmu::canonical(address))
}

/// Allocation-free wake after Page admission reopens. Pointer order is used only
/// for this bounded traversal, never as identity or authority. Removed mappings
/// have no live waiter; newly added waiters recheck the already-open predicate.
pub(crate) fn wake_all() {
    let mut cursor = 0;
    loop {
        let candidate = {
            let head = registry().head.lock();
            let mut node = head.as_deref();
            let mut next: Option<(usize, Weak<Span>)> = None;
            while let Some(record) = node {
                let key = record.mapping.as_ptr() as usize;
                if key > cursor && next.as_ref().is_none_or(|(known, _)| key < *known) {
                    next = Some((key, record.mapping.clone()));
                }
                node = record.next.as_deref();
            }
            next
        };
        let Some((key, candidate)) = candidate else {
            break;
        };
        cursor = key;
        let route = candidate.upgrade().and_then(|mapping| mapping.wake_key());
        if let Some((space, mapping)) = route {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::Mapping { space, mapping });
        }
    }
}
