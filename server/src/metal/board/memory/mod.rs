//! Validated physical memory facts, built without allocation before either heap.
mod ranges;
mod tree;

use super::Region;
use fack::prelude::Error;
use fdt::node::FdtNode;
use kernel_api::address::PAGE_SIZE;
use ranges::{Ranges, checked_end, covering_pages};

const MAX_RAM: usize = 16;
const MAX_RESERVED: usize = 64;
const MAX_FREE: usize = MAX_RAM + MAX_RESERVED;

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    #[error("malformed device tree envelope or structure")]
    Tree,
    #[error("invalid physical interval or address overflow")]
    Range,
    #[error("overlapping RAM banks or conflicting boot objects")]
    Overlap,
    #[error("unsupported or malformed memory description")]
    Description,
    #[error("early memory range capacity exceeded")]
    Capacity,
    #[error("boot object is not wholly contained in RAM")]
    OutsideRam,
    #[error("no allocatable pages remain")]
    Empty,
}

#[derive(Debug)]
pub struct Memory {
    ram: Ranges<MAX_RAM>,
    free: Ranges<MAX_FREE>,
    identity: Ranges<MAX_FREE>,
    dtb: Region,
    initrd: Option<Region>,
}

impl Memory {
    pub fn parse(bytes: &[u8], dtp: usize, kernel: Region) -> Result<Self, MemoryError> {
        let reservations = tree::validate(bytes)?;
        let fdt = fdt::Fdt::new(bytes).map_err(|_| MemoryError::Tree)?;
        let root = fdt.find_node("/").ok_or(MemoryError::Tree)?;
        let cells = (
            cells(root, "#address-cells", 2)?,
            cells(root, "#size-cells", 1)?,
        );
        for name in ["reserved-memory", "chosen"] {
            if root
                .children()
                .filter(|node| node.name.split('@').next() == Some(name))
                .count()
                > 1
            {
                return Err(MemoryError::Description);
            }
        }
        let mut ram = Ranges::new();
        for node in root.children() {
            if node.name.split('@').next() != Some("memory") {
                continue;
            }
            if !enabled(node)? {
                continue;
            }
            if property(node, "device_type")?.is_some_and(|p| p != b"memory\0") {
                return Err(MemoryError::Description);
            }
            regions(node, cells, |r| {
                if !r.base.is_multiple_of(PAGE_SIZE) || !r.size.is_multiple_of(PAGE_SIZE) {
                    return Err(MemoryError::Range);
                }
                ram.insert(r, false)
            })?;
        }
        if !ram.contains(kernel) {
            return Err(MemoryError::OutsideRam);
        }
        let dtb = Region::new(dtp, bytes.len());
        let dtb_pages = covering_pages(dtb)?;
        if !dtp.is_multiple_of(PAGE_SIZE) || !ram.contains(dtb_pages) {
            // The boot ledger exports whole pages and has no DTB offset field.
            return Err(MemoryError::OutsideRam);
        }
        let initrd = initrd(&fdt)?;
        if initrd.is_some_and(|r| !ram.contains(r)) {
            return Err(MemoryError::OutsideRam);
        }
        if overlaps(kernel, dtb)
            || initrd.is_some_and(|r| overlaps(r, kernel) || overlaps(r, dtb_pages))
        {
            return Err(MemoryError::Overlap);
        }
        let mut reserved: Ranges<MAX_RESERVED> = Ranges::new();
        for entry in bytes[reservations].chunks_exact(16) {
            if entry == [0; 16] {
                break;
            }
            reserved.insert(tree::reservation(entry)?, true)?;
        }
        let mut no_map: Ranges<MAX_RESERVED> = Ranges::new();
        if let Some(parent) = fdt.find_node("/reserved-memory") {
            if !enabled(parent)? {
                return Err(MemoryError::Description);
            }
            if (
                self::cells(parent, "#address-cells", 0)?,
                self::cells(parent, "#size-cells", 0)?,
            ) != cells
                || property(parent, "ranges")? != Some(&[][..])
            {
                return Err(MemoryError::Description);
            }
            for node in parent.children() {
                if enabled(node)? {
                    // Dynamic `size` reservations cannot be resolved before boot;
                    // do not silently treat their unknown storage as free memory.
                    let excluded = property(node, "no-map")?;
                    if excluded.is_some_and(|p| !p.is_empty()) {
                        return Err(MemoryError::Description);
                    }
                    regions(node, cells, |r| {
                        reserved.insert(r, true)?;
                        if excluded.is_some() {
                            no_map.insert(covering_pages(r)?, true)?;
                        }
                        Ok(())
                    })?;
                }
            }
        }
        if no_map.overlaps(dtb) || initrd.is_some_and(|r| no_map.overlaps(r)) {
            return Err(MemoryError::Overlap);
        }
        if reserved.overlaps(kernel) {
            return Err(MemoryError::Overlap);
        }
        // Null cannot back Rust allocations, even on platforms with RAM at 0.
        reserved.insert(Region::new(0, PAGE_SIZE), true)?;
        reserved.insert(kernel, true)?;
        reserved.insert(dtb, true)?;
        if let Some(initrd) = initrd {
            reserved.insert(initrd, true)?;
        }
        let free = ram.subtract(&reserved)?;
        free.envelope()?;
        Ok(Self {
            ram,
            free,
            identity: ram.subtract(&no_map)?,
            dtb: dtb_pages,
            initrd,
        })
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

fn property<'a>(node: FdtNode<'_, 'a>, name: &str) -> Result<Option<&'a [u8]>, MemoryError> {
    let mut properties = node.properties().filter(|p| p.name == name);
    let value = properties.next().map(|p| p.value);
    if properties.next().is_some() {
        return Err(MemoryError::Description);
    }
    Ok(value)
}

fn enabled(node: FdtNode<'_, '_>) -> Result<bool, MemoryError> {
    match property(node, "status")? {
        None | Some(b"ok\0" | b"okay\0") => Ok(true),
        Some(b"disabled\0" | b"reserved\0" | b"fail\0") => Ok(false),
        Some(s) if s.starts_with(b"fail-") && s.ends_with(&[0]) => Ok(false),
        _ => Err(MemoryError::Description),
    }
}

fn cells(node: FdtNode<'_, '_>, name: &str, default: usize) -> Result<usize, MemoryError> {
    let n = match property(node, name)? {
        None => default,
        Some(p) if p.len() == 4 => u32::from_be_bytes(p.try_into().unwrap()) as usize,
        _ => return Err(MemoryError::Description),
    };
    if matches!(n, 1 | 2) {
        Ok(n)
    } else {
        Err(MemoryError::Description)
    }
}

fn number(bytes: &[u8]) -> Result<usize, MemoryError> {
    let n = match bytes.len() {
        4 => u32::from_be_bytes(bytes.try_into().unwrap()) as u64,
        8 => u64::from_be_bytes(bytes.try_into().unwrap()),
        _ => return Err(MemoryError::Description),
    };
    usize::try_from(n).map_err(|_| MemoryError::Range)
}

fn regions(
    node: FdtNode<'_, '_>,
    (address, size): (usize, usize),
    mut take: impl FnMut(Region) -> Result<(), MemoryError>,
) -> Result<(), MemoryError> {
    let bytes = property(node, "reg")?.ok_or(MemoryError::Description)?;
    let stride = (address + size) * 4;
    if bytes.is_empty() || !bytes.len().is_multiple_of(stride) {
        return Err(MemoryError::Description);
    }
    for entry in bytes.chunks_exact(stride) {
        let r = Region::new(
            number(&entry[..address * 4])?,
            number(&entry[address * 4..])?,
        );
        checked_end(r)?;
        take(r)?;
    }
    Ok(())
}

fn initrd(fdt: &fdt::Fdt<'_>) -> Result<Option<Region>, MemoryError> {
    let Some(chosen) = fdt.find_node("/chosen") else {
        return Ok(None);
    };
    let start = property(chosen, "linux,initrd-start")?;
    let end = property(chosen, "linux,initrd-end")?;
    if start.is_none() && end.is_none() {
        return Ok(None);
    }
    let start = number(start.ok_or(MemoryError::Description)?)?;
    let end = number(end.ok_or(MemoryError::Description)?)?;
    if start == 0 || !start.is_multiple_of(PAGE_SIZE) || end <= start {
        return Err(MemoryError::Range);
    }
    Ok(Some(covering_pages(Region::new(start, end - start))?))
}
