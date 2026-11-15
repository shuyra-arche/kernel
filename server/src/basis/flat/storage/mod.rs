//! Backing and residency state for a linear source.

mod geometry;
mod lifecycle;
pub(super) mod materialize;
mod region;
pub(super) mod reshape;
pub(super) mod residency;
pub(super) mod slot;

use alloc::sync::Arc;
use alloc::vec::Vec;
pub use region::BorrowedRegion;

use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;

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
    pub geometry: geometry::Geometry,
    pub phase: kernel_api::model::flat::FlatPhase,
    pub explicit: bool,
    pub revision: residency::Revision,
    pub source: FlatSource,
    pub residencies: Vec<ResidencyEntry>,
    pub next_residency: usize,
    /// Original branch restrictions follow this holding after Allow as well.
    pub provenance: Vec<(PageIndex, Arc<crate::basis::pend::Provenance>)>,
    /// A staged holding retains its original maintenance record, even if the
    /// originating source loses its last public owner. This is no new authority.
    pub staged: Vec<(PageIndex, Arc<crate::basis::pend::Stage>)>,
    /// 这份来源是否还允许迁移。普通 `acquire` 建立时为真；导入/借用，
    /// 以及任何一次内核或全局发布都把它永久置假。
    pub transferable: bool,
}

impl FlatState {
    pub fn new(source: FlatSource, transferable: bool) -> Self {
        Self {
            geometry: geometry::Geometry::new(),
            phase: kernel_api::model::flat::FlatPhase::Open,
            explicit: false,
            revision: residency::Revision::new(),
            source,
            residencies: Vec::new(),
            next_residency: 1,
            staged: Vec::new(),
            provenance: Vec::new(),
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
            Slot::Allocating { .. } => true,
            Slot::Resident(page) => page.changing(),
            Slot::Missing => false,
        })
    }
}
