//! Protected PCI endpoint configuration and BAR lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Absent,
    Unsupported,
    Invalid,
    Busy,
    Hardware,
}

use crate::metal::generic::pci::Config;

pub(crate) struct Endpoint<C: Config> {
    config: C,
    identity: u32,
    size: usize,
    bar: usize,
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
        })
    }

    pub(crate) fn identity(&self) -> u32 {
        self.identity
    }
    pub(crate) fn base(&self) -> usize {
        self.bar
    }
    pub(crate) fn size(&self) -> usize {
        self.size
    }
    /// The bus allocator supplies an exclusive, identity-translated aperture.
    /// # Safety
    /// [base, base + size) is reserved for this BAR for the entire Endpoint
    /// lifetime, is disjoint from RAM, ECAM and protected MMIO, and is not exposed
    /// through another Unit mapping. No other device decodes this range.
    pub(crate) unsafe fn place(&mut self, base: usize) -> Result<(), Error> {
        if self.bar != 0 {
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
            return Err(Error::Hardware);
        }
        self.bar = base;
        Ok(())
    }

    /// Enable MMIO decode while bus mastering remains disabled.
    pub(crate) fn decode(&mut self) -> Result<(), Error> {
        self.command(0x402)
    }
    /// # Safety
    /// The trusted IOMMU projection is active before enabling bus mastering.
    pub(crate) unsafe fn activate(&mut self) -> Result<(), Error> {
        self.command(0x406)
    }
    /// Close admission only; this is never a DMA completion receipt.
    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        self.command(0x400)
    }
    fn command(&mut self, command: u16) -> Result<(), Error> {
        self.config.write16(4, command);
        self.config.sync();
        if self.config.read32(4) & 0x407 == u32::from(command) {
            Ok(())
        } else {
            Err(Error::Hardware)
        }
    }
}
