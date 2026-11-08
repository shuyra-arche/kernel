//! CPU translation and shootdown contracts.

pub(crate) mod fence;

use bitflags::bitflags;
use kernel_api::address::{PhysAddr, VirtAddr};

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

pub(crate) struct Root<I> {
    pub(crate) frame: PhysAddr,
    pub(crate) asid: I,
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
    type Error;

    fn root() -> Result<Self, Self::Error>;
    fn frame(&self) -> PhysAddr;
    fn prepare_geometry(
        &mut self,
        va: VirtAddr,
        size: usize,
        geo: Self::Geometry,
    ) -> Result<(), Self::Error>;
    fn prepare_range_geometry(
        &mut self,
        va: VirtAddr,
        pa: PhysAddr,
        size: usize,
        geo: Self::Geometry,
    ) -> Result<(), Self::Error>;
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
    ) -> Result<(PhysAddr, Attributes), Self::Error>;
    fn set_permissions_prepared_geometry(
        &mut self,
        va: VirtAddr,
        attrs: Attributes,
        geo: Self::Geometry,
    );
    fn unmap_prepared_geometry(&mut self, va: VirtAddr, size: usize, geo: Self::Geometry);
    fn reserve_retired_tables(
        &self,
        retired: &mut alloc::vec::Vec<Self>,
    ) -> Result<(), Self::Error>;
    fn reclaim_empty_tables(&mut self, retired: &mut alloc::vec::Vec<Self>);

    #[cfg(debug_assertions)]
    fn visit_leaves(
        &self,
        geo: Self::Geometry,
        visit: &mut impl FnMut(VirtAddr, PhysAddr, Attributes, usize),
    );
}

/// CPU execution identity and geometry are owned by the hardware backend.
/// PageTable prepares/commits edits; Shootdown certifies their exact covering request.
/// Both are selected together by metal. No general Nest path reads SATP or PTE encoding.
pub(crate) trait Domain: fence::Shootdown + Sized {
    type Identifier: Copy;
    type Geometry: Copy;
    type CreateError;
    fn create(instance: usize, kernel: bool) -> Result<Self, Self::CreateError>;
    fn identifier(&self) -> Self::Identifier;
    fn geometry(&self) -> Self::Geometry;
    fn publish(&self);
    fn published(&self) -> bool;
    fn released(&self) -> bool;
    fn release(&self) -> Result<(), Self::CreateError>;
}
