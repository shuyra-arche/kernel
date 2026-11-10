use crate::boot::ledger::ResourceKind;

pub const NAME_LEN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    Trap(Trap),
    Call(Call),
    Page(Page),
    Interrupt {
        controller: u64,
        source: u32,
    },
    /// Descriptive boot topology. Only the accompanying Pend grants access.
    Nest {
        controller: u64,
        requester: u32,
    },
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trap {
    /// Legacy descriptive name; version 6 never mints this global authority.
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
            Self::Trap(Trap::SupervisorExternal) | Self::Interrupt { .. } => ResourceKind::Sign,
            Self::Call(Call::Create) => ResourceKind::None,
            Self::Call(_) => ResourceKind::Control,
            Self::Trap(Trap::PageFault) => ResourceKind::Clone,
            Self::Page(_) => ResourceKind::Share,
            Self::Nest { .. } => ResourceKind::Nest,
        }
    }
}
impl Name {
    pub const WIDTH: usize = NAME_LEN;
    /// Write a ledger name into exactly `WIDTH` bytes.
    pub fn store(&self, out: &mut [u8]) {
        assert_eq!(out.len(), Self::WIDTH);
        let route = match *self {
            Self::Interrupt { controller, source } => Some((4, controller, source)),
            Self::Nest {
                controller,
                requester,
            } => Some((5, controller, requester)),
            _ => None,
        };
        if let Some((class, controller, source)) = route {
            out.fill(0);
            out[0] = class;
            out[4..8].copy_from_slice(&source.to_le_bytes());
            out[8..16].copy_from_slice(&controller.to_le_bytes());
            return;
        }
        let (class, item, at): (u8, u8, u64) = match *self {
            Self::Trap(Trap::SupervisorExternal) => (0, 0, 0),
            Self::Trap(Trap::PageFault) => (0, 1, 0),
            Self::Call(Call::Build) => (1, 0, 0),
            Self::Call(Call::Create) => (1, 4, 0),
            Self::Call(Call::Guard) => (1, 3, 0),
            Self::Page(Page::Dtb) => (2, 0, 0),
            Self::Page(Page::Initrd) => (2, 1, 0),
            Self::Page(Page::Region(base)) => (2, 2, base),
            Self::Interrupt { .. } | Self::Nest { .. } => unreachable!(),
        };
        out.fill(0);
        out[0] = class;
        out[1] = item;
        out[8..16].copy_from_slice(&at.to_le_bytes());
    }
    pub fn fetch(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..NAME_LEN)?;
        if matches!(bytes[0], 4 | 5) {
            let source = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
            let controller = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
            if bytes[1..4] != [0; 3] || controller == 0 {
                return None;
            }
            return match bytes[0] {
                4 if source != 0 => Some(Self::Interrupt { controller, source }),
                5 => Some(Self::Nest {
                    controller,
                    requester: source,
                }),
                _ => None,
            };
        }
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
            // Class 3 was Port and remains invalid.
            _ => None,
        }
    }
}
