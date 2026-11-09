/// Dense Kernel slot, independent of the firmware hart number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HartId(usize);

impl HartId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    pub const fn get(self) -> usize {
        self.0
    }

    pub const fn bit(self) -> (usize, usize) {
        (
            self.0 / usize::BITS as usize,
            1usize << (self.0 % usize::BITS as usize),
        )
    }
}

impl ::core::fmt::Display for HartId {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Hardware identity used by firmware, SBI and interrupt routing, never an array index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareId(usize);

impl FirmwareId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }
    pub const fn get(self) -> usize {
        self.0
    }

    /// One exact SBI target. Topology rejects usize::MAX, SBI's all-harts base.
    pub const fn target(self) -> (usize, usize) {
        (1, self.0)
    }
}
