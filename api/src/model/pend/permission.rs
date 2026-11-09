//! Operation permissions are typed by their target. They never identify an object.
//! Distribution is independent of data mutability; narrowing cannot change a target's kind.
use super::PendRange;
use crate::model::modifiable::Modifiable;
use crate::model::sign::SignRights;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Distributable {
    No = 0,
    Yes = 1,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CreationRights: u32 {
        const FLAT = 1;
        const NEST = 2;
        const CLONE = 4;
        const SHARE = 8;
        const SIGN = 16;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CloneRights: u32 {
        const PUSH = 1;
        const PULL = 2;
        const PEEK = 4;
        const DELETE = 8;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ShareRights: u32 {
        const OPEN = 1;
        const SHUT = 2;
        const ASSOCIATE = 4;
        const DELETE = 8;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FlatRights: u32 {
        const INSPECT = 1;
        const TRANSFER = 2;
        const ASSOCIATE = 4;
        const SHARE = 8;
        const DELETE = 16;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct NestRights: u32 {
        const INSPECT = 1;
        const RESERVE = 2;
        const RELEASE = 4;
        const ATTACH = 8;
        const DETACH = 16;
        const TRANSFER = 32;
        const PUBLISH = 64;
        const RETRACT = 128;
        const PROTECT = 256;
        const DELETE = 512;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PortRights: u32 {
        const INSPECT = 1;
        const ATTACH = 2;
    }
}

/// None carries permission for the listed object-free calls, not ambient access
/// to every object accepted by those calls. Existing-object rights name a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operations {
    None(CreationRights),
    Clone(CloneRights),
    Share(ShareRights),
    Flat(FlatRights),
    Nest(NestRights),
    Port(PortRights),
    Sign(SignRights),
}

impl Operations {
    pub const fn words(self) -> [usize; 2] {
        match self {
            Self::None(r) => [0, r.bits() as usize],
            Self::Clone(r) => [1, r.bits() as usize],
            Self::Share(r) => [2, r.bits() as usize],
            Self::Flat(r) => [3, r.bits() as usize],
            Self::Nest(r) => [4, r.bits() as usize],
            Self::Port(r) => [5, r.bits() as usize],
            Self::Sign(r) => [6, r.bits() as usize],
        }
    }
    pub fn of([kind, bits]: [usize; 2]) -> Option<Self> {
        let bits = u32::try_from(bits).ok()?;
        Some(match kind {
            0 => Self::None(CreationRights::from_bits(bits)?),
            1 => Self::Clone(CloneRights::from_bits(bits)?),
            2 => Self::Share(ShareRights::from_bits(bits)?),
            3 => Self::Flat(FlatRights::from_bits(bits)?),
            4 => Self::Nest(NestRights::from_bits(bits)?),
            5 => Self::Port(PortRights::from_bits(bits)?),
            6 => Self::Sign(SignRights::from_bits(bits)?),
            _ => return None,
        })
    }
    pub fn contains(self, child: Self) -> bool {
        let [kind, bits] = self.words();
        let [child_kind, child_bits] = child.words();
        kind == child_kind && bits & child_bits == child_bits
    }
}

/// A requested limit, not a copy of the Kernel's live authority record. The
/// server snapshots and validates it before committing a derivation/narrowing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub operations: Operations,
    pub modifiable: Modifiable,
    pub distributable: Distributable,
    pub range: PendRange,
}
impl Limits {
    pub const WORDS: usize = 7;
    pub fn words(self) -> [usize; Self::WORDS] {
        let [kind, operations] = self.operations.words();
        let range = match self.range {
            PendRange::Inherit => [0, 0, 0],
            PendRange::Pages { offset, count } => [1, offset, count],
        };
        [
            kind,
            operations,
            self.modifiable.bits() as usize,
            self.distributable as usize,
            range[0],
            range[1],
            range[2],
        ]
    }
    pub fn of(w: [usize; Self::WORDS]) -> Option<Self> {
        let result = Self {
            operations: Operations::of([w[0], w[1]])?,
            modifiable: Modifiable::from_bits(u32::try_from(w[2]).ok()?)?,
            distributable: match w[3] {
                0 => Distributable::No,
                1 => Distributable::Yes,
                _ => return None,
            },
            range: match [w[4], w[5], w[6]] {
                [0, 0, 0] => PendRange::Inherit,
                [1, offset, count] => PendRange::pages(offset, count)?,
                _ => return None,
            },
        };
        result.valid().then_some(result)
    }
    pub fn valid(self) -> bool {
        if matches!(
            self.operations,
            Operations::None(_) | Operations::Sign(_) | Operations::Port(_)
        ) && self.modifiable != Modifiable::No
        {
            return false;
        }
        match self.range {
            PendRange::Inherit => true,
            PendRange::Pages { offset, count } => {
                matches!(self.operations, Operations::Share(_) | Operations::Flat(_))
                    && PendRange::pages(offset, count).is_some()
            }
        }
    }
    /// Range coordinates are relative to the parent's effective window; the
    /// holder must resolve those under the same commit as this permission check.
    pub fn permits_child(self, child: Self) -> bool {
        self.valid()
            && child.valid()
            && self.distributable == Distributable::Yes
            && self.operations.contains(child.operations)
            && self.modifiable.allows(child.modifiable)
    }
    pub fn permits_narrowing(self, child: Self) -> bool {
        self.valid()
            && child.valid()
            && self.operations.contains(child.operations)
            && self.modifiable.allows(child.modifiable)
            && (self.distributable == Distributable::Yes
                || child.distributable == Distributable::No)
    }
}
