//! CPU physical coordinates for ordinary memory-mapped bus resources.
use super::{Error, cells, number, parent, property};
use alloc::vec::Vec;
use fdt::{Fdt, node::FdtNode};
use kernel_api::address::PAGE_SIZE;

mod region;
pub(crate) use region::Region;

#[derive(Clone, Copy)]
struct Span {
    base: usize,
    end: usize,
}

impl Span {
    fn new(base: usize, size: usize) -> Result<Self, Error> {
        let end = base
            .checked_add(size)
            .filter(|_| size != 0)
            .ok_or(Error::Description)?;
        Ok(Self { base, end })
    }

    fn within(self, outer: Self) -> bool {
        outer.base <= self.base && self.end <= outer.end
    }
    fn overlaps(self, other: Self) -> bool {
        self.base < other.end && other.base < self.end
    }

    fn width(self, cells: usize) -> Result<Self, Error> {
        if cells == 1 && self.end as u64 > 1u64 << 32 {
            return Err(Error::Description);
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
    fn through(&mut self, child: Span, parent: Span) -> Result<(), Error> {
        if !self.resource.within(child) {
            return Err(Error::Description);
        }
        let translate = |span: Span| -> Result<Span, Error> {
            let offset = span
                .base
                .checked_sub(child.base)
                .ok_or(Error::Description)?;
            Span::new(
                parent.base.checked_add(offset).ok_or(Error::Description)?,
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

    fn finish(self) -> Result<Region, Error> {
        let page = Span {
            base: self.resource.base / PAGE_SIZE * PAGE_SIZE,
            end: self
                .resource
                .end
                .checked_next_multiple_of(PAGE_SIZE)
                .ok_or(Error::Description)?,
        };
        if self.window.is_some_and(|window| !page.within(window)) {
            return Err(Error::Description);
        }
        Region::new(self.resource.base, self.resource.end - self.resource.base)
    }
}

fn address_cells(node: FdtNode<'_, '_>) -> Result<usize, Error> {
    let count = cells(node, "#address-cells", 2)?;
    if matches!(count, 1 | 2) {
        Ok(count)
    } else {
        Err(Error::Description)
    }
}

fn size_cells(node: FdtNode<'_, '_>) -> Result<usize, Error> {
    let count = cells(node, "#size-cells", 1)?;
    if matches!(count, 1 | 2) {
        Ok(count)
    } else {
        Err(Error::Description)
    }
}

/// Keep restricted IOMMU/interrupt backends on explicitly identity-mapped paths.
pub(crate) fn identity_path<'b, 'a>(
    fdt: &'b Fdt<'a>,
    mut bus: FdtNode<'b, 'a>,
) -> Result<(), Error> {
    while let Some(p) = parent(fdt, bus) {
        if property(bus, "ranges")? != Some(&[]) {
            return Err(Error::Description);
        }
        bus = p;
    }
    Ok(())
}

pub(crate) fn regions(fdt: &Fdt<'_>, node: FdtNode<'_, '_>) -> Result<Vec<Region>, Error> {
    let bus = parent(fdt, node).ok_or(Error::Description)?;
    let addresses = address_cells(bus)?;
    let sizes = size_cells(bus)?;
    let reg = property(node, "reg")?.ok_or(Error::Description)?;
    let stride = (addresses + sizes) * 4;
    if reg.is_empty() || !reg.len().is_multiple_of(stride) {
        return Err(Error::Description);
    }
    let mut result = Vec::new();
    result
        .try_reserve(reg.len() / stride)
        .map_err(|_| Error::OutOfMemory)?;
    for entry in reg.chunks_exact(stride) {
        let base = number(&entry[..addresses * 4])?;
        let size = number(&entry[addresses * 4..])?;
        result.push(physical(fdt, bus, base, size)?);
    }
    Ok(result)
}

/// Translate one range in `bus` coordinates through every ancestor.
pub(crate) fn physical<'b, 'a>(
    fdt: &'b Fdt<'a>,
    mut bus: FdtNode<'b, 'a>,
    base: usize,
    size: usize,
) -> Result<Region, Error> {
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
        let ranges = property(bus, "ranges")?.ok_or(Error::Description)?;
        if !ranges.is_empty() {
            let sizes = size_cells(bus)?;
            let stride = (addresses + parent_addresses + sizes) * 4;
            if !ranges.len().is_multiple_of(stride) {
                return Err(Error::Description);
            }
            let mut windows: Vec<(Span, Span)> = Vec::new();
            windows
                .try_reserve(ranges.len() / stride)
                .map_err(|_| Error::OutOfMemory)?;
            for entry in ranges.chunks_exact(stride) {
                let cut = addresses * 4;
                let end = cut + parent_addresses * 4;
                let size = number(&entry[end..])?;
                let child = Span::new(number(&entry[..cut])?, size)?.width(addresses)?;
                let parent = Span::new(number(&entry[cut..end])?, size)?.width(parent_addresses)?;
                if windows.iter().any(|(old, _)| child.overlaps(*old)) {
                    return Err(Error::Description);
                }
                windows.push((child, parent));
            }
            let (child, parent) = windows
                .into_iter()
                .find(|(child, _)| translation.resource.within(*child))
                .ok_or(Error::Description)?;
            translation.through(child, parent)?;
        }
        bus = p;
    }
}

/// One register span accepted by current controller backends. Identity-only is
/// a backend constraint, applied before the shared byte/page geometry decoder.
pub(crate) fn single(fdt: &Fdt, node: FdtNode) -> Result<Region, Error> {
    identity_path(fdt, parent(fdt, node).ok_or(Error::Description)?)?;
    let mut regions = regions(fdt, node)?;
    if regions.len() != 1 {
        return Err(Error::Description);
    }
    Ok(regions.pop().expect("one region"))
}
