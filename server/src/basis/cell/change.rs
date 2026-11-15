//! Find a Page's in-kernel admission gate. This is never an authority table:
//! callers must already hold the source and destination permissions. Memo freeze
//! objects own the gate; the registry holds only Weak references across Task death.
use super::page::RetainError;
use super::page::{Change, Page};
use crate::basis::sync::LegacyLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
struct Changes {
    entries: Vec<Weak<Change>>,
    next: usize,
    remaining: usize,
}
static LIVE: LegacyLock<crate::basis::sync::order::LiveChanges, Changes> =
    LegacyLock::new(Changes {
        entries: Vec::new(),
        next: 0,
        remaining: 0,
    });

/// All old and new physical gates commit with one exclusive receipt conversion.
/// Caller holds COMMIT, the maintenance record and the Flat holdings lock.
pub(crate) fn reshape(
    pages: &mut Vec<Arc<Page>>,
    blocks: &mut Vec<super::page::Block>,
    level: kernel_api::model::cell::CellLevel,
) -> Result<Vec<Arc<Page>>, super::page::PageError> {
    use super::page::{Block, PageError};
    let mut registry = LIVE.lock();
    let live = &mut registry.entries;
    for page in pages.iter() {
        let block = blocks
            .iter()
            .find(|b| Arc::ptr_eq(b.page(), page))
            .ok_or(PageError::Busy)?;
        let weak = live
            .iter()
            .filter(|w| w.as_ptr() == Arc::as_ptr(block.change()))
            .count();
        if !block.exclusive(page, weak) {
            return Err(PageError::Busy);
        }
    }
    let total = pages
        .iter()
        .try_fold(0usize, |n, p| n.checked_add(p.pages()))
        .ok_or(PageError::InvalidRange)?;
    let count = total / level.pages().ok_or(PageError::InvalidRange)?;
    let mut prepared = Vec::new();
    prepared
        .try_reserve(count)
        .map_err(|_| PageError::OutOfMemory)?;
    blocks
        .try_reserve(count)
        .map_err(|_| PageError::OutOfMemory)?;
    live.try_reserve(count)
        .map_err(|_| PageError::OutOfMemory)?;
    for _ in 0..count {
        prepared.push(Block::prepare().map_err(|_| PageError::OutOfMemory)?);
    }
    let result = Page::reshape_with(pages, level, |pages| {
        for page in pages {
            let at = blocks
                .iter()
                .position(|b| Arc::ptr_eq(b.page(), page))
                .unwrap();
            let mut block = blocks.remove(at);
            live.retain(|w| w.as_ptr() != Arc::as_ptr(block.change()));
            assert!(block.release_unique(page), "prevalidated exclusive gate");
        }
        Ok(())
    })?;
    for (page, mut block) in result.iter().zip(prepared) {
        block.bind(page);
        live.push(Arc::downgrade(block.change()));
        blocks.push(block);
    }
    Ok(result)
}

/// Caller holds COMMIT and the holding's Flat lock. Detach the registry's weak
/// upgrade path while proving exclusive ownership; restore it without allocation
/// on refusal. No hardware receipt is discharged by this operation.
pub(crate) fn release_block(
    mut block: super::page::Block,
    page: &mut Arc<Page>,
) -> Result<(), super::page::Block> {
    let mut registry = LIVE.lock();
    let live = &mut registry.entries;
    let at = live
        .iter()
        .position(|known| known.as_ptr() == Arc::as_ptr(block.change()));
    if let Some(at) = at {
        live.remove(at);
    }
    if block.release_unique(page) {
        drop(block);
        Ok(())
    } else {
        if let Some(at) = at {
            live.insert(at, Arc::downgrade(block.change()));
        }
        Err(block)
    }
}
pub(crate) fn acquire(page: &Arc<Page>) -> Result<Arc<Change>, RetainError> {
    let mut registry = LIVE.lock();
    let live = &mut registry.entries;
    live.retain(|change| change.strong_count() != 0);
    if let Some(change) = live
        .iter()
        .filter_map(Weak::upgrade)
        .find(|c| Arc::ptr_eq(c.page(), page))
    {
        return Ok(change);
    }
    live.try_reserve(1).map_err(|_| RetainError::Exhausted)?;
    let change = page.begin()?;
    live.push(Arc::downgrade(&change));
    Ok(change)
}

/// Collect the level-triggered completion after caller resource guards end.
/// The weak registry owns no Page authority; pruning has its own bounded cursor.
pub(crate) fn completed() -> bool {
    if super::page::reclaim::change::take_completion() {
        let mut registry = LIVE.lock();
        registry.remaining = registry.entries.len();
        true
    } else {
        false
    }
}

/// Inspect at most four weak gates. Concurrent acquire/release may shrink or
/// reorder discovery entries, so the cursor is normalized on every step. An
/// acquire already prunes every expired entry before admitting a new gate.
pub(crate) fn prune() -> bool {
    let mut registry = LIVE.lock();
    for _ in 0..4 {
        if registry.entries.is_empty() {
            registry.remaining = 0;
        }
        if registry.remaining == 0 {
            break;
        }
        let at = registry.next % registry.entries.len();
        if registry.entries[at].strong_count() == 0 {
            registry.entries.swap_remove(at);
            registry.next = at;
        } else {
            registry.next = at + 1;
        }
        registry.remaining -= 1;
    }
    registry.remaining != 0
}

pub(crate) fn pending() -> bool {
    LIVE.lock().remaining != 0
}
