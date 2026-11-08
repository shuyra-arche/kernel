//! Validated physical memory facts, built without allocation before either heap.
mod ranges;

use super::Region;
use crate::address::PAGE_SIZE;
use ranges::{Ranges, checked_end, covering_pages};

const MAX_RAM: usize = 16;
const MAX_RESERVED: usize = 64;
const MAX_FREE: usize = MAX_RAM + MAX_RESERVED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    Range,
    Overlap,
    Description,
    Capacity,
    OutsideRam,
    Empty,
}

#[derive(Debug)]
pub struct Memory {
    ram: Ranges<MAX_RAM>,
    free: Ranges<MAX_FREE>,
    identity: Ranges<MAX_FREE>,
    kernel: Region,
    dtb: Region,
    initrd: Option<Region>,
}

impl Memory {
    /// Admit raw platform facts before either allocator exists. All intervals
    /// are bounded and checked here; firmware decoding stays with the caller.
    pub fn new(
        ram: &[Region],
        reserved: &[Region],
        no_map: &[Region],
        dtb: Region,
        kernel: Region,
        initrd: Option<Region>,
    ) -> Result<Self, MemoryError> {
        let mut banks = Ranges::<MAX_RAM>::new();
        for r in ram {
            if !r.base.is_multiple_of(PAGE_SIZE) || !r.size.is_multiple_of(PAGE_SIZE) {
                return Err(MemoryError::Range);
            }
            banks.insert(*r, false)?;
        }
        let ram = banks;
        checked_end(kernel)?;
        if !ram.contains(kernel) {
            return Err(MemoryError::OutsideRam);
        }
        let dtb_pages = covering_pages(dtb)?;
        if !dtb.base.is_multiple_of(PAGE_SIZE) || !ram.contains(dtb_pages) {
            return Err(MemoryError::OutsideRam);
        }
        if let Some(r) = initrd {
            if r.base == 0 || !r.base.is_multiple_of(PAGE_SIZE) || !r.size.is_multiple_of(PAGE_SIZE)
            {
                return Err(MemoryError::Range);
            }
            if !ram.contains(r) {
                return Err(MemoryError::OutsideRam);
            }
        }
        if overlaps(kernel, dtb_pages)
            || initrd.is_some_and(|r| overlaps(r, kernel) || overlaps(r, dtb_pages))
        {
            return Err(MemoryError::Overlap);
        }
        let mut holes = Ranges::<MAX_RESERVED>::new();
        for r in reserved {
            holes.insert(*r, true)?;
        }
        let mut excluded = Ranges::<MAX_RESERVED>::new();
        for r in no_map {
            let r = covering_pages(*r)?;
            excluded.insert(r, true)?;
            holes.insert(r, true)?;
        }
        let no_map = excluded;
        let mut reserved = holes;
        if no_map.overlaps(dtb_pages)
            || initrd.is_some_and(|r| no_map.overlaps(r))
            || reserved.overlaps(kernel)
        {
            return Err(MemoryError::Overlap);
        }
        reserved.insert(Region::new(0, PAGE_SIZE), true)?;
        reserved.insert(kernel, true)?;
        reserved.insert(dtb_pages, true)?;
        if let Some(initrd) = initrd {
            reserved.insert(initrd, true)?;
        }
        let free = ram.subtract(&reserved)?;
        free.envelope()?;
        Ok(Self {
            ram,
            free,
            identity: ram.subtract(&no_map)?,
            kernel,
            dtb: dtb_pages,
            initrd,
        })
    }
    /// Prevent a caller from describing another image's reservation.
    pub fn protects(&self, image: Region) -> bool {
        self.kernel == image
    }

    pub fn identity(&self) -> &[Region] {
        self.identity.as_slice()
    }
    pub fn free(&self) -> &[Region] {
        self.free.as_slice()
    }
    /// Bounds for indexing and conservative exclusion only; holes are not RAM.
    pub fn envelope(&self) -> Region {
        self.ram.envelope().expect("validated RAM")
    }
    pub fn allocation_bounds(&self) -> Region {
        self.free.envelope().expect("validated free pages")
    }
    pub fn bootstrap(&self) -> Region {
        *self
            .free()
            .iter()
            .max_by_key(|r| r.size)
            .expect("validated free pages")
    }
    pub fn contains_identity(&self, region: Region) -> bool {
        self.identity.contains(region)
    }
    pub fn dtb(&self) -> Region {
        self.dtb
    }
    pub fn initrd(&self) -> Option<Region> {
        self.initrd
    }
}

fn overlaps(a: Region, b: Region) -> bool {
    a.base < b.range().end && b.base < a.range().end
}

impl core::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Range => "invalid physical interval",
            Self::Overlap => "overlapping physical ownership",
            Self::Description => "invalid memory description",
            Self::Capacity => "admission capacity exceeded",
            Self::OutsideRam => "boot object outside RAM",
            Self::Empty => "no allocatable memory",
        })
    }
}
impl core::error::Error for MemoryError {}
