use super::super::{slot, slot::Slot, source::FlatSource, state::FlatState};
use crate::basis::cell::operation::TransferError;
use crate::basis::cell::page::Page;
use alloc::sync::Arc;
use kernel_api::address::{PageCount, PageIndex};
/// 两端共同的来源资格：这一份来源还允许迁移。
///
/// 资格是来源自己的**永久**状态，不靠“现在有没有关系压着它”现算：导入、借用，
/// 以及任何一次内核或全局发布都会把它整份置假——全局条目对 ASID 定向失效免疫，
/// 旧 TLB 无从确认，而关联退场并不能证明它没了。
pub(super) fn plain_origin(state: &FlatState) -> Result<(), TransferError> {
    if !state.transferable {
        return Err(TransferError::NotTransferable);
    }
    Ok(())
}

/// 这一页是否被驻留覆盖：两端都一样，覆盖就当场 Busy。
pub(super) fn pinned(state: &FlatState, page: PageIndex) -> bool {
    state.is_pinned(page, PageCount::new(1).expect("one page"))
}

/// 源端：来源仍可迁、无驻留，且这一页真的是空闲的承载。
pub(super) fn source_ready(state: &FlatState, page: PageIndex) -> Result<(), TransferError> {
    plain_origin(state)?;
    if pinned(state, page) {
        return Err(TransferError::Busy);
    }
    let FlatSource::Managed(slots) = &state.source else {
        return Err(TransferError::NotTransferable);
    };
    let (at, present) = slot::spot(slots, page);
    if !present {
        return Err(TransferError::SourceEmpty);
    }
    match slots[at].1 {
        Slot::Resident(_) => Ok(()),
        Slot::Missing => Err(TransferError::SourceEmpty),
        Slot::Moving { .. } | Slot::Reserved { .. } => Err(TransferError::Busy),
    }
}

/// 目标端：来源仍可迁、无驻留，且这一页是空的（Fresh 或 `Missing` 墓碑）。
///
/// 返回值是“目标原本是不是墓碑”：回退时要原样还回去，不能把墓碑抹成 Fresh。
pub(super) fn target_ready(state: &FlatState, page: PageIndex) -> Result<bool, TransferError> {
    plain_origin(state)?;
    if pinned(state, page) {
        return Err(TransferError::Busy);
    }
    let FlatSource::Managed(slots) = &state.source else {
        return Err(TransferError::NotTransferable);
    };
    let (at, present) = slot::spot(slots, page);
    if !present {
        return Ok(false);
    }
    match slots[at].1 {
        Slot::Resident(_) => Err(TransferError::DestinationOccupied),
        Slot::Missing => Ok(true),
        Slot::Moving { .. } | Slot::Reserved { .. } => Err(TransferError::Busy),
    }
}

/// 目标槽的容量：在取得承载独占**之前**备齐，于是 OOM 不会留下在途槽。
pub(super) fn reserve(state: &mut FlatState, page: PageIndex) -> Result<(), TransferError> {
    let FlatSource::Managed(slots) = &mut state.source else {
        return Err(TransferError::NotTransferable);
    };
    let (_, present) = slot::spot(slots, page);
    if present {
        return Ok(());
    }
    slots.try_reserve(1).map_err(|_| TransferError::OutOfMemory)
}

/// 取走源槽的承载：`Resident → Moving`；来源锁内封闭该槽的新物化与发布。
pub(super) fn claim_source(state: &mut FlatState, page: PageIndex, id: usize) {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the source was checked to be a plain acquired source")
    };
    let (at, present) = slot::spot(slots, page);
    assert!(present, "the source page was checked to hold a carrier");
    let cell = match &slots[at].1 {
        Slot::Resident(cell) => cell.clone(),
        _ => unreachable!("the source page was checked to be an idle carrier"),
    };
    slots[at].1 = Slot::Moving {
        cell,
        transaction: id,
    };
}

/// 目标槽进保留：`Fresh | Missing → Reserved`（容量已在 `reserve` 备好）。
pub(super) fn claim_target(state: &mut FlatState, page: PageIndex, id: usize, was_missing: bool) {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the target was checked to be a plain acquired source")
    };
    let (at, present) = slot::spot(slots, page);
    if present {
        assert!(
            was_missing && matches!(slots[at].1, Slot::Missing),
            "an occupied target slot is exactly the tombstone that was checked"
        );
        slots[at].1 = Slot::Reserved {
            transaction: id,
            was_missing,
        };
        return;
    }
    assert!(!was_missing, "an absent target slot is fresh");
    slots.insert(
        at,
        (
            page,
            Slot::Reserved {
                transaction: id,
                was_missing: false,
            },
        ),
    );
}

/// 结算源槽：`Moving → Missing`；交出来的正是这一笔占住的那个帧。
pub(super) fn settle_source(state: &mut FlatState, page: PageIndex, id: usize) -> Arc<Page> {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the source was a plain acquired source when it was claimed")
    };
    let (at, present) = slot::spot(slots, page);
    assert!(present, "a claimed source slot keeps its seat");
    let cell = match &slots[at].1 {
        Slot::Moving { cell, transaction } => {
            assert!(
                *transaction == id,
                "the source slot belongs to this transaction"
            );
            cell.clone()
        }
        _ => panic!("a claimed source slot stays Moving"),
    };
    slots[at].1 = Slot::Missing;
    cell
}

/// 结算目标槽：`Reserved → Resident`；同一 Page 改由目标槽持有。
pub(super) fn settle_target(state: &mut FlatState, page: PageIndex, id: usize, cell: Arc<Page>) {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the target was a plain acquired source when it was claimed")
    };
    let (at, present) = slot::spot(slots, page);
    assert!(present, "a reserved target slot keeps its seat");
    match &slots[at].1 {
        Slot::Reserved { transaction, .. } => assert!(
            *transaction == id,
            "the target slot is the reservation this transaction made"
        ),
        _ => panic!("a reserved target slot stays Reserved"),
    }
    slots[at].1 = Slot::Resident(cell);
}

/// 回退源槽：`Moving → Resident`；同一 Page 恢复源槽的正常持有。
pub(super) fn rollback_source(state: &mut FlatState, page: PageIndex, id: usize) {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the source was a plain acquired source when it was claimed")
    };
    let (at, present) = slot::spot(slots, page);
    assert!(present, "a claimed source slot keeps its seat");
    let cell = match &slots[at].1 {
        Slot::Moving { cell, transaction } => {
            assert!(
                *transaction == id,
                "the source slot belongs to this transaction"
            );
            cell.clone()
        }
        _ => panic!("a claimed source slot stays Moving"),
    };
    slots[at].1 = Slot::Resident(cell);
}

/// 回退目标槽：把空位还回它原本的样子（Fresh 就摘掉、`Missing` 就留墓碑）。
pub(super) fn rollback_target(state: &mut FlatState, page: PageIndex, id: usize) {
    let FlatSource::Managed(slots) = &mut state.source else {
        unreachable!("the target was a plain acquired source when it was claimed")
    };
    let (at, present) = slot::spot(slots, page);
    assert!(present, "a reserved target slot keeps its seat");
    let was_missing = match slots[at].1 {
        Slot::Reserved {
            transaction,
            was_missing,
        } => {
            assert!(
                transaction == id,
                "the target slot is the reservation this transaction made"
            );
            was_missing
        }
        _ => panic!("a reserved target slot stays Reserved"),
    };
    if was_missing {
        slots[at].1 = Slot::Missing;
    } else {
        slots.remove(at);
    }
}
