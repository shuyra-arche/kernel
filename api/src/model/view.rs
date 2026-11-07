//! Cross-Unit data interaction. View identity alone never authorizes memory access.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewId(usize);
impl ViewId {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Share = 0,
    Clone = 2,
}
impl ViewKind {
    pub const fn of(value: usize) -> Option<Self> {
        match value {
            0 => Some(Self::Share),
            2 => Some(Self::Clone),
            _ => None,
        }
    }
}
