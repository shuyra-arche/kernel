//! RISC-V IOMMU/PLIC admission, checked before any controller MMIO effects.
use super::super::resources::{contains, footprint, overlaps};
use super::super::{Memory, Region, Resources};
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
/// Selected RV64 controls plus provider-independent resource custody.
#[derive(Clone, Copy)]
pub struct Devices<'a> {
    pub resources: Resources<'a>,
    pub controllers: &'a [Region],
    pub interrupt: Option<Plic<'a>>,
}
impl Devices<'_> {
    pub fn validate(&self, memory: &Memory) -> Result<(), PendFail> {
        self.resources.validate(memory)?;
        if self.controllers.len() > 1 || self.controllers.is_empty() != self.interrupt.is_none() {
            return Err(PendFail::Denied);
        }
        for r in self.controllers {
            footprint(*r)?;
            if !r.base.is_multiple_of(PAGE_SIZE)
                || r.size != PAGE_SIZE
                || !self.resources.protected.iter().any(|p| contains(*p, *r))
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
                || !self
                    .resources
                    .protected
                    .iter()
                    .any(|r| contains(*r, region))
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
}
