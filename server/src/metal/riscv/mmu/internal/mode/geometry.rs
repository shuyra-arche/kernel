use riscv::register::satp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geo {
    pub levels: u8,
    pub va_bits: u8,
}

impl Geo {
    #[inline]
    pub fn split_bit(self) -> u8 {
        self.va_bits - 1
    }

    /// Highest RISC-V leaf level: L2 for Sv39, L3 for Sv48 (512 GiB),
    /// and L4 for Sv57 (256 TiB). This is not physical Cell geometry.
    pub fn hardware_max_leaf_level(self) -> usize {
        usize::from(self.levels) - 1
    }

    /// Automatic mapping deliberately stops at L2 (1 GiB) for all modes.
    /// Higher leaf sizes require independent preparation/shootdown validation.
    pub fn automatic_leaf_level(self) -> usize {
        self.hardware_max_leaf_level().min(2)
    }
}

pub fn geometry(mode: satp::Mode) -> Geo {
    match mode {
        satp::Mode::Sv39 => Geo {
            levels: 3,
            va_bits: 39,
        },
        satp::Mode::Sv48 => Geo {
            levels: 4,
            va_bits: 48,
        },
        satp::Mode::Sv57 => Geo {
            levels: 5,
            va_bits: 57,
        },
        _ => panic!("geometry: unsupported satp mode {mode:?}"),
    }
}
