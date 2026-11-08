//! 一次移交的短事务：两端页槽在同一个临界区里进入在途态。
//!
//! 事务只改页槽，不复制数据、不分配数据帧、不碰 PTE/TLB。
//! 守卫活着的时候两端就一直是那个在途态：源头是 `Moving`（自己强持原 `Page`），
//! 目标是 `Reserved`（记着原本是 Fresh 还是 `Missing`）。于是提前返回、`?` 或
//! panic=abort 都不会让承载失去归属——它始终在源槽里。到点 `commit` 消费守卫，
//! 没 commit 就被 Drop 的按同一笔身份原样回退两端。
//!
//! 同一份来源只取一次锁；不同来源按地址序共持一对（`machine::sync::with_pair`）。
//! 可失败的步骤（两端资格、两端驻留与在途、目标槽容量，以及在这之后、写槽之前的
//! 纯 core 准备回调）全在任何槽被改动之前做完；随后只有承载独占与写槽，写槽不再
//! 返回失败。
//!
//! 每个 Page 只会经消费式导入或新分配进入一个槽。Arc<Page> 快照不能重新导入，
//! 因此 Moving / Reserved 与来源锁就是槽迁移的唯一状态；映射引用不构成第二个槽。

use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;

use crate::basis::cell::transfer::TransferError;
use crate::machine::sync::with_pair;
use kernel_api::address::PageIndex;

use super::Flat;
use super::FlatRange;
use super::slot;
use super::slot::Slot;
use super::state::FlatSource;
use super::state::FlatState;

#[derive(PartialEq, Eq)]
enum Phase {
    Claimed,
    Committed,
}

/// 一次移交：两端各自的页已经换成本来源上的绝对页号。
pub(crate) struct Transaction {
    source: Arc<Flat>,
    source_page: PageIndex,
    target: Arc<Flat>,
    target_page: PageIndex,
    id: usize,
    phase: Phase,
    change: Arc<crate::basis::cell::page::Change>,
}

impl Transaction {
    /// 短双锁里占槽：源 `Resident → Moving`，目标 `Fresh | Missing → Reserved`。
    ///
    /// 两端资格、pin、页槽与目标容量这些可失败的检查全做完之后、任何 `Moving`/
    /// `Reserved` 槽或承载独占被写之前，调一次纯 core 的 `prepare`；它自己可失败，
    /// 失败时守卫尚未构造、也没有一格槽被改动。成功后守卫才连同 `prepare` 的载荷
    /// 一起交出，于是占槽与它引用的元数据来自同一个临界区。
    pub(crate) fn claim<C>(
        source: &FlatRange,
        source_page: PageIndex,
        target: &FlatRange,
        target_page: PageIndex,
        change: Option<&Arc<crate::basis::cell::page::Change>>,
        prepare: impl FnOnce() -> Result<C, TransferError>,
    ) -> Result<(Self, C), TransferError> {
        assert!(
            owns(source, source_page),
            "the source page is inside its window"
        );
        assert!(
            owns(target, target_page),
            "the target page is inside its window"
        );
        let same = Arc::ptr_eq(source.flat(), target.flat());
        if same && source_page == target_page {
            return Err(TransferError::DestinationOccupied);
        }
        let id = next_transaction()?;
        let source = source.flat().clone();
        let target = target.flat().clone();
        let carried = if same {
            let mut state = source.state.lock();
            state.source_ready(source_page)?;
            let was_missing = state.target_ready(target_page)?;
            state.reserve(target_page)?;
            let carried = prepare()?;
            let gate = claim_page(&state, source_page, change)?;
            state.claim_source(source_page, id);
            state.claim_target(target_page, id, was_missing);
            (carried, gate)
        } else {
            with_pair(&source.state, &target.state, |from, to| {
                from.source_ready(source_page)?;
                let was_missing = to.target_ready(target_page)?;
                to.reserve(target_page)?;
                let carried = prepare()?;
                let gate = claim_page(from, source_page, change)?;
                from.claim_source(source_page, id);
                to.claim_target(target_page, id, was_missing);
                Ok((carried, gate))
            })?
        };
        let transaction = Self {
            source,
            source_page,
            target,
            target_page,
            id,
            phase: Phase::Claimed,
            change: carried.1,
        };
        Ok((transaction, carried.0))
    }

    /// 结算并消费守卫：源成墓碑，目标拿到同一个承载；之后 Drop 不再回退。
    pub(crate) fn commit(mut self) {
        self.settle();
        self.phase = Phase::Committed;
    }

    /// 两端在同一临界区里结算：`Moving → Missing`，`Reserved → Resident`。
    fn settle(&self) {
        debug_assert!(self.change.page().changing());
        if Arc::ptr_eq(&self.source, &self.target) {
            let mut state = self.source.state.lock();
            let cell = state.settle_source(self.source_page, self.id);
            state.settle_target(self.target_page, self.id, cell);
        } else {
            with_pair(&self.source.state, &self.target.state, |from, to| {
                let cell = from.settle_source(self.source_page, self.id);
                to.settle_target(self.target_page, self.id, cell);
            });
        }
    }
}

impl Drop for Transaction {
    /// 没有提交就被放掉：两端在同一笔身份下原样还回去。
    fn drop(&mut self) {
        if self.phase == Phase::Committed {
            return;
        }
        if Arc::ptr_eq(&self.source, &self.target) {
            let mut state = self.source.state.lock();
            state.rollback_source(self.source_page, self.id);
            state.rollback_target(self.target_page, self.id);
        } else {
            with_pair(&self.source.state, &self.target.state, |from, to| {
                from.rollback_source(self.source_page, self.id);
                to.rollback_target(self.target_page, self.id);
            });
        }
    }
}

/// 页落在窗口里：入口已经查过，这里是不变量。
fn owns(range: &FlatRange, page: PageIndex) -> bool {
    page.get() >= range.index().get() && page.get() - range.index().get() < range.page_count().get()
}

/// 一次移交的事务身份：同一笔的两端一致，用来认自己的在途槽。
///
/// 号耗尽（或超出符号上界）就当场失败，不回收旧号。
fn next_transaction() -> Result<usize, TransferError> {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    crate::machine::sync::next_id(&NEXT).ok_or(TransferError::OutOfMemory)
}

fn claim_page(
    state: &FlatState,
    index: PageIndex,
    change: Option<&Arc<crate::basis::cell::page::Change>>,
) -> Result<Arc<crate::basis::cell::page::Change>, TransferError> {
    let FlatSource::Managed(slots) = &state.source else {
        return Err(TransferError::NotTransferable);
    };
    let (at, present) = slot::spot(slots, index);
    if !present {
        return Err(TransferError::SourceEmpty);
    }
    let Slot::Resident(page) = &slots[at].1 else {
        return Err(TransferError::Busy);
    };
    match change {
        Some(change) if Arc::ptr_eq(change.page(), page) => Ok(change.clone()),
        Some(_) => Err(TransferError::Busy),
        None => page.begin().map_err(|_| TransferError::Busy),
    }
}
