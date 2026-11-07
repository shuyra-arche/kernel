#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendStatus {
    Active,
    Suspended,
    Retiring,
    Dead,
}
impl PendStatus {
    pub const fn of(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::Active),
            1 => Some(Self::Suspended),
            2 => Some(Self::Retiring),
            3 => Some(Self::Dead),
            _ => None,
        }
    }
}
