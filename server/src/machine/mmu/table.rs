use bitflags::bitflags;
use kernel_api::address::{PhysAddr, VirtAddr};

use crate::basis::nest::NestError;

use super::identity::Asid;

bitflags! {
    /// Meaning of a valid leaf. Validity and physical encoding belong to the backend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Attributes: u64 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
        const USER = 1 << 3;
        const GLOBAL = 1 << 4;
        const ACCESSED = 1 << 5;
        const DIRTY = 1 << 6;
        const PERMISSIONS = Self::READ.bits() | Self::WRITE.bits() | Self::EXECUTE.bits();
    }
}

pub(crate) struct Root {
    pub(crate) frame: PhysAddr,
    pub(crate) asid: Asid,
}

/// CPU translation storage. Prepare can allocate tables or split a leaf while
/// preserving translations. Commit runs under the Nest lock, without allocation.
/// Detached tables remain owned by Nest until the covering fence completes.
///
/// # Safety
/// Implementations must publish only the specified frames and permissions,
/// report actual valid leaves, and retain all detached tables for the caller.
pub(crate) unsafe trait PageTable: Sized {
    type Geometry: Copy;

    fn root() -> Result<Self, NestError>;
    fn frame(&self) -> PhysAddr;
    fn prepare_geometry(
        &mut self,
        va: VirtAddr,
        size: usize,
        geo: Self::Geometry,
    ) -> Result<(), NestError>;
    fn prepare_range_geometry(
        &mut self,
        va: VirtAddr,
        pa: PhysAddr,
        size: usize,
        geo: Self::Geometry,
    ) -> Result<(), NestError>;
    fn map_range_prepared_geometry(
        &mut self,
        va: VirtAddr,
        pa: PhysAddr,
        size: usize,
        attrs: Attributes,
        geo: Self::Geometry,
    );
    fn walk_ref_geometry(
        &self,
        va: VirtAddr,
        geo: Self::Geometry,
    ) -> Result<(PhysAddr, Attributes), NestError>;
    fn set_permissions_prepared_geometry(
        &mut self,
        va: VirtAddr,
        attrs: Attributes,
        geo: Self::Geometry,
    );
    fn unmap_prepared_geometry(&mut self, va: VirtAddr, size: usize, geo: Self::Geometry);
    fn reserve_retired_tables(&self, retired: &mut alloc::vec::Vec<Self>) -> Result<(), NestError>;
    fn reclaim_empty_tables(&mut self, retired: &mut alloc::vec::Vec<Self>);

    #[cfg(debug_assertions)]
    fn visit_leaves(
        &self,
        geo: Self::Geometry,
        visit: &mut impl FnMut(VirtAddr, PhysAddr, Attributes, usize),
    );
}
