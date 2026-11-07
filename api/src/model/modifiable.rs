//! Pend's only access choice. Possession grants reading; modification requires Yes.
//! Delegation never adds access, and borrowing rules are enforced by Kernel state.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifiable {
    No = 0,
    Yes = 1,
}
impl Modifiable {
    pub const fn bits(self) -> u32 {
        self as u32
    }
    pub const fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => Some(Self::No),
            1 => Some(Self::Yes),
            _ => None,
        }
    }
    pub const fn allows(self, requested: Self) -> bool {
        matches!(self, Self::Yes) || matches!(requested, Self::No)
    }
}
