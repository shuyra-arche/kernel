use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct PteFlags: u64 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

impl PteFlags {
    pub(super) fn from_attributes(attrs: crate::metal::mmu::Attributes) -> Self {
        Self::from_bits_truncate(attrs.bits() << 1)
    }

    pub(super) fn attributes(self) -> crate::metal::mmu::Attributes {
        crate::metal::mmu::Attributes::from_bits_truncate(self.bits() >> 1)
    }
}
