//! CPU physical coordinates for ordinary memory-mapped bus resources.
use alloc::vec::Vec;
use fdt::{Fdt, node::FdtNode};
use kernel_api::{abi::call::pend::PendFail, address::PAGE_SIZE};

use super::region::Region;

#[derive(Clone, Copy)]
struct Span {
    base: usize,
    end: usize,
}

impl Span {
    fn new(base: usize, size: usize) -> Result<Self, PendFail> {
        let end = base
            .checked_add(size)
            .filter(|_| size != 0)
            .ok_or(PendFail::Denied)?;
        Ok(Self { base, end })
    }

    fn within(self, outer: Self) -> bool {
        outer.base <= self.base && self.end <= outer.end
    }
    fn overlaps(self, other: Self) -> bool {
        self.base < other.end && other.base < self.end
    }

    fn width(self, cells: usize) -> Result<Self, PendFail> {
        if cells == 1 && self.end as u64 > 1u64 << 32 {
            return Err(PendFail::Denied);
        }
        Ok(self)
    }
}

/// Carry the intersection of every ancestor window along with the resource.
/// Only the final CPU page footprint is checked: rounding intermediate bus
/// coordinates would be wrong when a translation changes the page offset.
struct Translation {
    resource: Span,
    window: Option<Span>,
}

impl Translation {
    fn through(&mut self, child: Span, parent: Span) -> Result<(), PendFail> {
        if !self.resource.within(child) {
            return Err(PendFail::Denied);
        }
        let translate = |span: Span| -> Result<Span, PendFail> {
            let offset = span.base.checked_sub(child.base).ok_or(PendFail::Denied)?;
            Span::new(
                parent.base.checked_add(offset).ok_or(PendFail::Denied)?,
                span.end - span.base,
            )
        };
        let window = self.window.map_or(child, |window| Span {
            base: window.base.max(child.base),
            end: window.end.min(child.end),
        });
        self.resource = translate(self.resource)?;
        self.window = Some(translate(window)?);
        Ok(())
    }

    fn finish(self) -> Result<Region, PendFail> {
        let page = Span {
            base: self.resource.base / PAGE_SIZE * PAGE_SIZE,
            end: self
                .resource
                .end
                .checked_next_multiple_of(PAGE_SIZE)
                .ok_or(PendFail::Denied)?,
        };
        if self.window.is_some_and(|window| !page.within(window)) {
            return Err(PendFail::Denied);
        }
        Region::new(self.resource.base, self.resource.end - self.resource.base)
    }
}

pub(super) fn property<'a>(
    node: FdtNode<'_, 'a>,
    name: &str,
) -> Result<Option<&'a [u8]>, PendFail> {
    let mut values = node.properties().filter(|p| p.name == name);
    let value = values.next().map(|p| p.value);
    if values.next().is_some() {
        return Err(PendFail::Denied);
    }
    Ok(value)
}

pub(super) fn number(bytes: &[u8]) -> Result<usize, PendFail> {
    if !matches!(bytes.len(), 4 | 8) {
        return Err(PendFail::Denied);
    }
    bytes.iter().try_fold(0usize, |n, b| {
        n.checked_mul(256)
            .and_then(|n| n.checked_add(*b as usize))
            .ok_or(PendFail::Denied)
    })
}

pub(super) fn cells(node: FdtNode<'_, '_>, name: &str, default: usize) -> Result<usize, PendFail> {
    match property(node, name)? {
        None => Ok(default),
        Some(bytes) if bytes.len() == 4 => number(bytes),
        _ => Err(PendFail::Denied),
    }
}

fn address_cells(node: FdtNode<'_, '_>) -> Result<usize, PendFail> {
    let count = cells(node, "#address-cells", 2)?;
    if matches!(count, 1 | 2) {
        Ok(count)
    } else {
        Err(PendFail::Denied)
    }
}

fn size_cells(node: FdtNode<'_, '_>) -> Result<usize, PendFail> {
    let count = cells(node, "#size-cells", 1)?;
    if matches!(count, 1 | 2) {
        Ok(count)
    } else {
        Err(PendFail::Denied)
    }
}

pub(super) fn parent<'b, 'a>(fdt: &'b Fdt<'a>, node: FdtNode<'b, 'a>) -> Option<FdtNode<'b, 'a>> {
    // Names have distinct storage even when equal under different parents.
    fdt.all_nodes()
        .find(|p| p.children().any(|c| core::ptr::eq(c.name, node.name)))
}

pub(super) fn enabled<'b, 'a>(
    fdt: &'b Fdt<'a>,
    mut node: FdtNode<'b, 'a>,
) -> Result<bool, PendFail> {
    loop {
        match property(node, "status")? {
            None | Some(b"okay\0" | b"ok\0") => {}
            Some(b"disabled\0" | b"reserved\0" | b"fail\0") => return Ok(false),
            Some(s) if s.starts_with(b"fail-") && s.ends_with(&[0]) => return Ok(false),
            _ => return Err(PendFail::Denied),
        }
        match parent(fdt, node) {
            Some(p) => node = p,
            None => return Ok(true),
        }
    }
}

/// CPU IDs, reserved-memory and PCI child encodings are not ordinary MMIO.
/// A bus with zero size cells describes identifiers, not memory apertures.
pub(super) fn ordinary<'b, 'a>(fdt: &'b Fdt<'a>, node: FdtNode<'b, 'a>) -> Result<bool, PendFail> {
    let mut current = node;
    loop {
        if matches!(
            current.name.split('@').next(),
            Some("cpus" | "reserved-memory" | "memory")
        ) {
            return Ok(false);
        }
        let Some(bus) = parent(fdt, current) else {
            return Ok(true);
        };
        if super::ranges::is_pci_host(bus) || cells(bus, "#size-cells", 1)? == 0 {
            return Ok(false);
        }
        current = bus;
    }
}

/// Keep restricted IOMMU/interrupt backends on explicitly identity-mapped paths.
pub(super) fn identity_path<'b, 'a>(
    fdt: &'b Fdt<'a>,
    mut bus: FdtNode<'b, 'a>,
) -> Result<(), PendFail> {
    while let Some(p) = parent(fdt, bus) {
        if property(bus, "ranges")? != Some(&[]) {
            return Err(PendFail::Denied);
        }
        bus = p;
    }
    Ok(())
}

pub(super) fn regions(fdt: &Fdt<'_>, node: FdtNode<'_, '_>) -> Result<Vec<Region>, PendFail> {
    let bus = parent(fdt, node).ok_or(PendFail::Denied)?;
    let addresses = address_cells(bus)?;
    let sizes = size_cells(bus)?;
    let reg = property(node, "reg")?.ok_or(PendFail::Denied)?;
    let stride = (addresses + sizes) * 4;
    if reg.is_empty() || !reg.len().is_multiple_of(stride) {
        return Err(PendFail::Denied);
    }
    let mut result = Vec::new();
    result
        .try_reserve(reg.len() / stride)
        .map_err(|_| PendFail::OutOfMemory)?;
    for entry in reg.chunks_exact(stride) {
        let base = number(&entry[..addresses * 4])?;
        let size = number(&entry[addresses * 4..])?;
        result.push(physical(fdt, bus, base, size)?);
    }
    Ok(result)
}

/// Translate one range in `bus` coordinates through every ancestor.
pub(super) fn physical<'b, 'a>(
    fdt: &'b Fdt<'a>,
    mut bus: FdtNode<'b, 'a>,
    base: usize,
    size: usize,
) -> Result<Region, PendFail> {
    let mut translation = Translation {
        resource: Span::new(base, size)?,
        window: None,
    };
    loop {
        let addresses = address_cells(bus)?;
        translation.resource.width(addresses)?;
        let Some(p) = parent(fdt, bus) else {
            return translation.finish();
        };
        let parent_addresses = address_cells(p)?;
        let ranges = property(bus, "ranges")?.ok_or(PendFail::Denied)?;
        if !ranges.is_empty() {
            let sizes = size_cells(bus)?;
            let stride = (addresses + parent_addresses + sizes) * 4;
            if !ranges.len().is_multiple_of(stride) {
                return Err(PendFail::Denied);
            }
            let mut windows: Vec<(Span, Span)> = Vec::new();
            windows
                .try_reserve(ranges.len() / stride)
                .map_err(|_| PendFail::OutOfMemory)?;
            for entry in ranges.chunks_exact(stride) {
                let cut = addresses * 4;
                let end = cut + parent_addresses * 4;
                let size = number(&entry[end..])?;
                let child = Span::new(number(&entry[..cut])?, size)?.width(addresses)?;
                let parent = Span::new(number(&entry[cut..end])?, size)?.width(parent_addresses)?;
                if windows.iter().any(|(old, _)| child.overlaps(*old)) {
                    return Err(PendFail::Denied);
                }
                windows.push((child, parent));
            }
            let (child, parent) = windows
                .into_iter()
                .find(|(child, _)| translation.resource.within(*child))
                .ok_or(PendFail::Denied)?;
            translation.through(child, parent)?;
        }
        bus = p;
    }
}
