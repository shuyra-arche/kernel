/// A Sign handle is resolved only in the calling Task's table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignId(usize);
impl SignId {
    pub const fn new(raw: usize) -> Self {
        Self(raw)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

bitflags::bitflags! {
    /// Independent control-flow authority; no Cell access or Unit control follows.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SignRights: u8 {
        const LISTEN = 1;
        const TRIGGER = 2;
        const ROUTE = 4;
    }
}
