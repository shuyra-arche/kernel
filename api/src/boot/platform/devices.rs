//! Physical ownership and normalized controller routes. No bus enumeration.
use super::{Memory, Region};
use crate::{abi::call::pend::PendFail, address::PAGE_SIZE};
#[derive(Clone, Copy)]
pub struct Plic<'a> {
    pub base: usize,
    pub size: usize,
    pub sources: usize,
    pub contexts: &'a [usize],
    pub selected: usize,
    pub queues: [u32; 3],
}
#[derive(Clone, Copy)]
pub struct Devices<'a> {
    pub authorities: &'a [Region],
    /// Controls which may never be delegated (including firmware-owned aliases).
    pub protected: &'a [Region],
    /// Exclusively held windows from which trusted external resources may be offered.
    pub external: &'a [Region],
    pub controllers: &'a [Region],
    pub interrupt: Option<Plic<'a>>,
}
fn footprint(r: Region) -> Result<Region, PendFail> {
    if r.base == 0 || r.size == 0 {
        return Err(PendFail::Denied);
    }
    let end = r
        .base
        .checked_add(r.size)
        .and_then(|n| n.checked_next_multiple_of(PAGE_SIZE))
        .ok_or(PendFail::Denied)?;
    let start = r.base / PAGE_SIZE * PAGE_SIZE;
    Ok(Region::new(start, end - start))
}
fn overlaps(a: Region, b: Region) -> bool {
    match (footprint(a), footprint(b)) {
        (Ok(a), Ok(b)) => a.base < b.base + b.size && b.base < a.base + a.size,
        _ => true,
    }
}
fn contains(a: Region, b: Region) -> bool {
    match (footprint(a), footprint(b)) {
        (Ok(a), Ok(b)) => a.base <= b.base && b.base + b.size <= a.base + a.size,
        _ => false,
    }
}
impl Devices<'_> {
    pub fn validate(&self, memory: &Memory) -> Result<(), PendFail> {
        let ram = memory.envelope();
        for list in [
            self.authorities,
            self.protected,
            self.external,
            self.controllers,
        ] {
            if list.len() > 4096 {
                return Err(PendFail::Denied);
            }
            for r in list {
                footprint(*r)?;
                if overlaps(*r, ram) {
                    return Err(PendFail::Denied);
                }
            }
        }
        for (i, r) in self.authorities.iter().enumerate() {
            if self.authorities[..i]
                .iter()
                .chain(self.protected)
                .chain(self.external)
                .any(|p| overlaps(*p, *r))
            {
                return Err(PendFail::Denied);
            }
        }
        for (i, r) in self.external.iter().enumerate() {
            if self.external[..i]
                .iter()
                .chain(self.protected)
                .any(|p| overlaps(*p, *r))
            {
                return Err(PendFail::Denied);
            }
        }
        if self.controllers.len() > 1 || self.controllers.is_empty() != self.interrupt.is_none() {
            return Err(PendFail::Denied);
        }
        for r in self.controllers {
            if !r.base.is_multiple_of(PAGE_SIZE)
                || r.size != PAGE_SIZE
                || !self.protected.iter().any(|p| contains(*p, *r))
            {
                return Err(PendFail::Denied);
            }
        }
        if let Some(p) = self.interrupt {
            let region = Region::new(p.base, p.size);
            footprint(region)?;
            if !p.base.is_multiple_of(PAGE_SIZE)
                || !(1..=1023).contains(&p.sources)
                || p.contexts.is_empty()
                || p.contexts.len() > 512
                || !p.contexts.contains(&p.selected)
                || !self.protected.iter().any(|r| contains(*r, region))
                || self.controllers.iter().any(|r| overlaps(*r, region))
            {
                return Err(PendFail::Denied);
            }
            for (i, context) in p.contexts.iter().enumerate() {
                if p.contexts[..i].contains(context)
                    || context
                        .checked_mul(0x1000)
                        .and_then(|n| n.checked_add(0x200008))
                        .is_none_or(|n| n > p.size)
                    || context
                        .checked_mul(0x80)
                        .and_then(|n| n.checked_add(0x2000 + (p.sources / 32 + 1) * 4))
                        .is_none_or(|n| n > p.size)
                {
                    return Err(PendFail::Denied);
                }
            }
            for (i, irq) in p.queues.iter().enumerate() {
                if *irq == 0 || *irq as usize > p.sources || p.queues[..i].contains(irq) {
                    return Err(PendFail::Denied);
                }
            }
        }
        Ok(())
    }
    /// Caller first validates the entire description. Admission of an external
    /// window then checks its exact page footprint, rather than a bus identifier.
    pub fn admits_external(&self, region: Region) -> Result<(), PendFail> {
        if !region.base.is_multiple_of(PAGE_SIZE) || !region.size.is_multiple_of(PAGE_SIZE) {
            return Err(PendFail::Denied);
        }
        footprint(region)?;
        if !self.external.iter().any(|r| contains(*r, region))
            || self
                .protected
                .iter()
                .chain(self.authorities)
                .any(|r| overlaps(*r, region))
        {
            return Err(PendFail::Denied);
        }
        Ok(())
    }
}
