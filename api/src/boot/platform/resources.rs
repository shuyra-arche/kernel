//! Provider-independent physical MMIO custody, without controller identities.
//! Validation is bounded admission of supplied facts, not a hardware probe or a
//! grant of authority. The selected boot provider must validate its own routes
//! before Kernel publishes any resource.
use super::{Memory, Region};
use crate::{abi::call::pend::PendFail, address::PAGE_SIZE};
#[derive(Clone, Copy)]
pub struct Resources<'a> {
    pub authorities: &'a [Region],
    /// Controls which may never be delegated (including firmware-owned aliases).
    pub protected: &'a [Region],
    /// Exclusively held windows from which trusted external resources may be offered.
    pub external: &'a [Region],
}
impl Resources<'_> {
    pub fn validate(&self, memory: &Memory) -> Result<(), PendFail> {
        let ram = memory.envelope();
        for list in [self.authorities, self.protected, self.external] {
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

pub(super) fn footprint(r: Region) -> Result<Region, PendFail> {
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
pub(super) fn overlaps(a: Region, b: Region) -> bool {
    match (footprint(a), footprint(b)) {
        (Ok(a), Ok(b)) => a.base < b.base + b.size && b.base < a.base + a.size,
        _ => true,
    }
}
pub(super) fn contains(a: Region, b: Region) -> bool {
    match (footprint(a), footprint(b)) {
        (Ok(a), Ok(b)) => a.base <= b.base && b.base + b.size <= a.base + a.size,
        _ => false,
    }
}
