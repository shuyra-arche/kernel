use crate::boot::ledger::ResourceKind;

pub const NAME_LEN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    Trap(Trap),
    Call(Call),
    Page(Page),
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trap {
    SupervisorExternal,
    PageFault,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Call {
    Build,
    Create,
    /// Boot-minted authority for the kernel execution-domain guard.
    Guard,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Dtb,
    Initrd,
    Region(u64),
}

impl Name {
    pub const fn base(self) -> Option<u64> {
        match self {
            Self::Page(Page::Region(base)) => Some(base),
            _ => None,
        }
    }
    pub const fn kind(self) -> ResourceKind {
        match self {
            Self::Trap(Trap::SupervisorExternal) => ResourceKind::Sign,
            Self::Call(Call::Create) => ResourceKind::None,
            Self::Call(_) => ResourceKind::Control,
            Self::Trap(Trap::PageFault) => ResourceKind::Clone,
            Self::Page(_) => ResourceKind::Share,
        }
    }
}
impl Name {
    pub const WIDTH: usize = NAME_LEN;
    /// Write a ledger name into exactly `WIDTH` bytes.
    pub fn store(&self, out: &mut [u8]) {
        assert_eq!(out.len(), Self::WIDTH);
        let (class, item, at): (u8, u8, u64) = match *self {
            Self::Trap(Trap::SupervisorExternal) => (0, 0, 0),
            Self::Trap(Trap::PageFault) => (0, 1, 0),
            Self::Call(Call::Build) => (1, 0, 0),
            Self::Call(Call::Create) => (1, 4, 0),
            Self::Call(Call::Guard) => (1, 3, 0),
            Self::Page(Page::Dtb) => (2, 0, 0),
            Self::Page(Page::Initrd) => (2, 1, 0),
            Self::Page(Page::Region(base)) => (2, 2, base),
        };
        out.fill(0);
        out[0] = class;
        out[1] = item;
        out[8..16].copy_from_slice(&at.to_le_bytes());
    }
    pub fn fetch(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..NAME_LEN)?;
        if bytes[2..8].iter().any(|b| *b != 0) {
            return None;
        }
        let at = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        match (bytes[0], bytes[1], at) {
            (0, 0, 0) => Some(Self::Trap(Trap::SupervisorExternal)),
            (0, 1, 0) => Some(Self::Trap(Trap::PageFault)),
            (1, 0, 0) => Some(Self::Call(Call::Build)),
            (1, 4, 0) => Some(Self::Call(Call::Create)),
            (1, 3, 0) => Some(Self::Call(Call::Guard)),
            (2, 0, 0) => Some(Self::Page(Page::Dtb)),
            (2, 1, 0) => Some(Self::Page(Page::Initrd)),
            (2, 2, base) if base != 0 => Some(Self::Page(Page::Region(base))),
            _ => None,
        }
    }
}
