//! Restricted conventional PCI endpoint. No raw config write interface exists.
//! Bridges, multifunction devices, PCIe/ATS/PRI/SR-IOV, ROM, IO/64-bit BARs and
//! MSI-X need their own isolation protocol and are deliberately not admitted.
use super::{Phase, phase::Event};
use crate::core::memory::nest::iommu::retirement::{Admission, Owner};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Absent,
    Unsupported,
    Invalid,
    Busy,
    Denied,
    Hardware,
}

/// # Safety
/// One fixed function's configuration, exclusively held by Kernel. Accesses use
/// exact widths, are ordered, and remain valid until the endpoint is retired.
/// Device topology and routing cannot change behind this object. Reading and
/// writing standard config registers has the PCI-defined effects.
pub(crate) unsafe trait Config {
    fn read32(&self, offset: usize) -> u32;
    fn write16(&mut self, offset: usize, value: u16);
    fn write32(&mut self, offset: usize, value: u32);
    fn sync(&self);
}

pub(crate) struct Endpoint<C: Config> {
    config: C,
    identity: u32,
    size: usize,
    bar: usize,
    phase: Phase,
    owner: Option<Admission>,
}

impl<C: Config> Endpoint<C> {
    /// Discover while DMA/decode are disabled. Only the standard MSI capability
    /// is accepted and disabled; interrupt delivery is not delegated by config.
    /// Failure never enables DMA or leaves a BAR sizing write behind.
    pub(crate) fn discover(mut config: C) -> Result<Self, Error> {
        let identity = config.read32(0);
        if identity as u16 == 0xffff || identity as u16 == 0 {
            return Err(Error::Absent);
        }
        // Exact 16-bit command writes avoid clearing W1C status bits.
        config.write16(4, 1 << 10);
        config.sync();
        if config.read32(4) & 7 != 0 {
            return Err(Error::Hardware);
        }
        if (config.read32(12) >> 16) as u8 != 0 {
            return Err(Error::Unsupported);
        }
        if !matches!(config.read32(0x100), 0 | u32::MAX) {
            return Err(Error::Unsupported);
        }
        if config.read32(0x30) & !0x7ff != 0 {
            return Err(Error::Unsupported);
        }
        if config.read32(4) & (1 << 20) != 0 {
            let mut at = (config.read32(0x34) & 0xff) as usize;
            let mut seen = [false; 64];
            while at != 0 {
                if at < 0x40 || at > 0xfc || at % 4 != 0 || seen[at / 4] {
                    return Err(Error::Unsupported);
                }
                seen[at / 4] = true;
                let cap = config.read32(at);
                if cap & 0xff != 5 {
                    return Err(Error::Unsupported);
                }
                // Reject malformed MSI layout before accessing the control word.
                let control = (cap >> 16) as u16;
                let bytes = if control & (1 << 7) != 0 { 14 } else { 10 };
                let bytes = bytes + if control & (1 << 8) != 0 { 10 } else { 0 };
                if at + bytes > 256 {
                    return Err(Error::Unsupported);
                }
                config.write16(at + 2, control & !1);
                config.sync();
                if config.read32(at) & (1 << 16) != 0 {
                    return Err(Error::Hardware);
                }
                at = ((cap >> 8) & 0xff) as usize;
            }
        }
        let original = config.read32(0x10);
        if original & 0xf != 0 {
            return Err(Error::Unsupported);
        }
        for offset in (0x14..0x28).step_by(4) {
            let old = config.read32(offset);
            config.write32(offset, u32::MAX);
            let mask = config.read32(offset);
            config.write32(offset, old);
            config.sync();
            if mask != 0 {
                return Err(Error::Unsupported);
            }
        }
        config.write32(0x10, u32::MAX);
        let mask = config.read32(0x10);
        config.write32(0x10, original);
        config.sync();
        if mask & 0xf != 0 || mask == 0 {
            return Err(Error::Unsupported);
        }
        let size = (!(mask & !0xf)).wrapping_add(1) as usize;
        if size < 4096 || !size.is_power_of_two() {
            return Err(Error::Unsupported);
        }
        Ok(Self {
            config,
            identity,
            size,
            bar: 0,
            phase: Phase::Available,
            owner: None,
        })
    }

    pub(crate) fn identity(&self) -> u32 {
        self.identity
    }
    pub(crate) fn size(&self) -> usize {
        self.size
    }
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    /// The bus allocator supplies an exclusive, identity-translated aperture.
    /// # Safety
    /// [base, base + size) is reserved for this BAR for the entire Endpoint
    /// lifetime, is disjoint from RAM, ECAM and protected MMIO, and is not exposed
    /// through another Unit mapping. No other device decodes this range.
    pub(crate) unsafe fn place(&mut self, base: usize) -> Result<(), Error> {
        if self.phase != Phase::Available || self.bar != 0 {
            return Err(Error::Busy);
        }
        if base == 0
            || !base.is_multiple_of(self.size)
            || base
                .checked_add(self.size)
                .is_none_or(|end| end > 1usize << 32)
        {
            return Err(Error::Invalid);
        }
        self.config.write32(0x10, base as u32);
        self.config.sync();
        if self.config.read32(0x10) != base as u32 {
            self.fail();
            return Err(Error::Hardware);
        }
        self.bar = base;
        Ok(())
    }

    pub(crate) fn claim(&mut self, owner: &Owner) -> Result<(), Error> {
        if self.bar == 0 || owner.closed() {
            return Err(Error::Denied);
        }
        let phase = self.phase.transition(Event::Claim).ok_or(Error::Busy)?;
        self.owner = Some(owner.admission());
        self.phase = phase;
        self.config.write16(4, (1 << 10) | 2);
        self.config.sync();
        if self.config.read32(4) & 0x407 != 0x402 {
            self.fail();
            return Err(Error::Hardware);
        }
        Ok(())
    }

    fn authorize(&self, owner: &Owner) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Owned | Phase::Active)
            || !self.owner.as_ref().is_some_and(|held| held.accepts(owner))
        {
            return Err(Error::Denied);
        }
        Ok(())
    }

    pub(crate) fn owned_by(&self, owner: &Owner) -> bool {
        self.owner.as_ref().is_some_and(|held| held.accepts(owner))
    }

    /// Only immutable standard-header fields. Capability space is never a
    /// caller-selected effects channel and no config write is exposed.
    pub(crate) fn inspect(&self, owner: &Owner, offset: usize) -> Result<u32, Error> {
        self.authorize(owner)?;
        if !matches!(offset, 0 | 8 | 12 | 0x2c) {
            return Err(Error::Denied);
        }
        Ok(self.config.read32(offset))
    }

    /// Resolve one BAR access after checking the actual owner and bounds. The
    /// executor must keep its serialization guard until the MMIO effect ends.
    pub(crate) fn access(
        &self,
        owner: &Owner,
        offset: usize,
        width: usize,
    ) -> Result<usize, Error> {
        self.authorize(owner)?;
        if !matches!(width, 1 | 2 | 4 | 8)
            || !offset.is_multiple_of(width)
            || offset.checked_add(width).is_none_or(|end| end > self.size)
        {
            return Err(Error::Invalid);
        }
        Ok(self.bar + offset)
    }

    /// # Safety
    /// The matching controller/DID has a retained, Active IoSpace projection;
    /// the caller keeps that responsibility until this endpoint stops admission.
    /// Device identity, routing and absence of bypass/ATS remain trusted.
    pub(crate) unsafe fn activate(&mut self, owner: &Owner) -> Result<(), Error> {
        self.authorize(owner)?;
        let next = self.phase.transition(Event::Activate).ok_or(Error::Busy)?;
        self.phase = next; // partial effects are already live responsibility
        self.config.write16(4, (1 << 10) | 6);
        self.config.sync();
        if self.config.read32(4) & 0x407 != 0x406 {
            self.fail();
            return Err(Error::Hardware);
        }
        Ok(())
    }

    /// No Unit cooperation, allocations or busy-wait. This is admission closure,
    /// NOT a proof that prior DMA completed or that any Cell is reclaimable.
    pub(crate) fn retire(&mut self) -> Result<(), Error> {
        let next = self.phase.transition(Event::Retire).ok_or(Error::Denied)?;
        self.phase = next;
        self.config.write16(4, 1 << 10);
        self.config.sync();
        if self.config.read32(4) & 7 != 0 {
            self.fail();
            return Err(Error::Hardware);
        }
        Ok(())
    }

    pub(crate) fn owner_closed(&self) -> bool {
        self.owner.as_ref().is_some_and(Admission::closed)
    }

    /// # Safety
    /// All device projections, platform in-flight effects and CPU BAR access
    /// have settled. Even then the one-shot endpoint is not reassigned.
    pub(crate) unsafe fn settle(&mut self) -> Result<(), Error> {
        self.phase = self.phase.transition(Event::Settle).ok_or(Error::Denied)?;
        Ok(())
    }

    fn fail(&mut self) {
        self.phase = self.phase.transition(Event::Fail).unwrap_or(self.phase);
    }
}
