//! Backing and residency state for a linear source.

mod region;
pub(super) mod residency;
pub(super) mod slot;

use alloc::sync::Arc;
use alloc::vec::Vec;
pub use region::BorrowedRegion;

use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;

use crate::basis::cell::page::Page;
use crate::basis::flat::transfer::TransferError;

use self::slot::Slot;

/// Flat 的物理来源。两种来源共用范围选择与物化协议，回收规则不同：
/// Managed 的页由分配器回收，Borrowed 的存储不归 RAM 分配器。
pub(super) enum FlatSource {
    /// 逻辑页槽，按页号严格递增；缺席的页是 Fresh（不占槽）。
    Managed(Vec<(PageIndex, Slot)>),
    /// 外部保证存活的物理区间；不分配其数据页、不清零、不按 RAM 回收。
    Borrowed(Arc<BorrowedRegion>),
}

pub(super) struct ResidencyEntry {
    pub id: usize,
    pub index: PageIndex,
    pub count: PageCount,
}

pub(super) struct FlatState {
    pub explicit: bool,
    pub revision: residency::Revision,
    pub source: FlatSource,
    pub residencies: Vec<ResidencyEntry>,
    pub next_residency: usize,
    /// 这份来源是否还允许迁移。普通 `acquire` 建立时为真；导入/借用，
    /// 以及任何一次内核或全局发布都把它永久置假。
    pub transferable: bool,
}

impl FlatState {
    pub fn new(source: FlatSource, transferable: bool) -> Self {
        Self {
            explicit: false,
            revision: residency::Revision::new(),
            source,
            residencies: Vec::new(),
            next_residency: 1,
            transferable,
        }
    }

    pub fn is_pinned(&self, index: PageIndex, count: PageCount) -> bool {
        let end = index.get() + count.get();
        self.residencies.iter().any(|entry| {
            entry.index.get() < end && index.get() < entry.index.get() + entry.count.get()
        })
    }

    /// 窗口里有没有页正被一次移交持有/预留：那些页此时没有可交出去的承载。
    pub fn busy(&self, index: PageIndex, count: PageCount) -> bool {
        let FlatSource::Managed(slots) = &self.source else {
            return false;
        };
        let end = index.get() + count.get();
        let first = slots.partition_point(|(slot, _)| slot.get() < index.get());
        let last = slots.partition_point(|(slot, _)| slot.get() < end);
        slots[first..last].iter().any(|(_, state)| match state {
            Slot::Moving { .. } | Slot::Reserved { .. } => true,
            Slot::Resident(page) => page.changing(),
            Slot::Missing => false,
        })
    }

    /// 两端共同的来源资格：这一份来源还允许迁移。
    ///
    /// 资格是来源自己的**永久**状态，不靠“现在有没有关系压着它”现算：导入、借用，
    /// 以及任何一次内核或全局发布都会把它整份置假——全局条目对 ASID 定向失效免疫，
    /// 旧 TLB 无从确认，而关联退场并不能证明它没了。
    fn plain_origin(&self) -> Result<(), TransferError> {
        if !self.transferable || self.explicit {
            return Err(TransferError::NotTransferable);
        }
        Ok(())
    }

    /// 这一页是否被驻留覆盖：两端都一样，覆盖就当场 Busy。
    fn pinned(&self, page: PageIndex) -> bool {
        self.is_pinned(page, PageCount::new(1).expect("one page"))
    }

    /// 源端：来源仍可迁、无驻留，且这一页真的是空闲的承载。
    pub(super) fn source_ready(&self, page: PageIndex) -> Result<(), TransferError> {
        self.plain_origin()?;
        if self.pinned(page) {
            return Err(TransferError::Busy);
        }
        let FlatSource::Managed(slots) = &self.source else {
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
    pub(super) fn target_ready(&self, page: PageIndex) -> Result<bool, TransferError> {
        self.plain_origin()?;
        if self.pinned(page) {
            return Err(TransferError::Busy);
        }
        let FlatSource::Managed(slots) = &self.source else {
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
    pub(super) fn reserve(&mut self, page: PageIndex) -> Result<(), TransferError> {
        let FlatSource::Managed(slots) = &mut self.source else {
            return Err(TransferError::NotTransferable);
        };
        let (_, present) = slot::spot(slots, page);
        if present {
            return Ok(());
        }
        slots.try_reserve(1).map_err(|_| TransferError::OutOfMemory)
    }

    /// 取走源槽的承载：`Resident → Moving`；来源锁内封闭该槽的新物化与发布。
    pub(super) fn claim_source(&mut self, page: PageIndex, id: usize) {
        let FlatSource::Managed(slots) = &mut self.source else {
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
    pub(super) fn claim_target(&mut self, page: PageIndex, id: usize, was_missing: bool) {
        let FlatSource::Managed(slots) = &mut self.source else {
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
    pub(super) fn settle_source(&mut self, page: PageIndex, id: usize) -> Arc<Page> {
        let FlatSource::Managed(slots) = &mut self.source else {
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
    pub(super) fn settle_target(&mut self, page: PageIndex, id: usize, cell: Arc<Page>) {
        let FlatSource::Managed(slots) = &mut self.source else {
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
    pub(super) fn rollback_source(&mut self, page: PageIndex, id: usize) {
        let FlatSource::Managed(slots) = &mut self.source else {
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
    pub(super) fn rollback_target(&mut self, page: PageIndex, id: usize) {
        let FlatSource::Managed(slots) = &mut self.source else {
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
}
