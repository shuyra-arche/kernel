/// Task-local administrative qualification; never a memory borrow or event right.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlId(usize);
impl ControlId {
    pub const NONE: Self = Self(0);
    pub const fn new(raw: usize) -> Self {
        Self(raw)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Build,
    Doom,
    Device,
    Retain,
    Guard,
}
