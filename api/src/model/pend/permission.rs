//! Operation permissions are typed by their target. They never identify an object.
//! Distribution is independent of data mutability; narrowing cannot change a target's kind.
use super::{NestScope, PendScope};
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
        const BLOCK = 32;
        #[cfg(feature = "memory-v11")]
        const OCCUPY = 64;
        #[cfg(feature = "memory-v11")]
        const VACATE = 128;
        #[cfg(feature = "memory-v11")]
        const RESHAPE = 256;
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct NestRights: u32 {
        const INSPECT = 1;
        const RESERVE = 2;
        const RELEASE = 4;
        /// Adopt an empty, exclusive Internal Nest when constructing a Team.
        /// This does not authorize external source discovery or binding.
        const ATTACH = 8;
        const PUBLISH = 64;
        const RETRACT = 128;
        const PROTECT = 256;
        const DELETE = 512;
        const INSERT = 1024;
        const REMOVE = 2048;
        #[cfg(feature = "memory-v11")]
        const FAULT = 4096;
    }
}

impl ShareRights {
    /// Source use in the selected wire contract; v11 publishes only through Nest.
    pub const fn association() -> Self {
        if cfg!(feature = "memory-v11") {
            Self::ASSOCIATE
        } else {
            Self::OPEN.union(Self::SHUT).union(Self::ASSOCIATE)
        }
    }
}

impl NestRights {
    pub const fn whole() -> Self {
        Self::INSPECT
            .union(Self::RESERVE)
            .union(Self::ATTACH)
            .union(Self::DELETE)
    }

    pub const fn reservation() -> Self {
        let rights = Self::INSPECT
            .union(Self::RELEASE)
            .union(Self::PUBLISH)
            .union(Self::RETRACT)
            .union(Self::PROTECT)
            .union(Self::INSERT)
            .union(Self::REMOVE);
        #[cfg(feature = "memory-v11")]
        let rights = rights.union(Self::FAULT);
        rights
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
            Self::Sign(r) => [6, r.bits() as usize],
        }
    }
    pub fn of([kind, bits]: [usize; 2]) -> Option<Self> {
        let bits = u32::try_from(bits).ok()?;
        // Open/Shut are retired; v11 Share only selects logical sources.
        if cfg!(feature = "memory-v11") && kind == 2 && bits & 3 != 0 {
            return None;
        }
        Some(match kind {
            0 => Self::None(CreationRights::from_bits(bits)?),
            1 => Self::Clone(CloneRights::from_bits(bits)?),
            2 => Self::Share(ShareRights::from_bits(bits)?),
            3 => Self::Flat(FlatRights::from_bits(bits)?),
            4 => Self::Nest(NestRights::from_bits(bits)?),
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
    pub scope: PendScope,
}
impl Limits {
    pub const WORDS: usize = 9;
    pub fn words(self) -> [usize; Self::WORDS] {
        let [kind, operations] = self.operations.words();
        let mut words = [0; Self::WORDS];
        words[..4].copy_from_slice(&[
            kind,
            operations,
            self.modifiable.bits() as usize,
            self.distributable as usize,
        ]);
        words[4..].copy_from_slice(&self.scope.words());
        words
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
            scope: PendScope::of(w[4..].try_into().ok()?)?,
        };
        result.valid().then_some(result)
    }
    pub fn valid(self) -> bool {
        if Operations::of(self.operations.words()) != Some(self.operations)
            || PendScope::of(self.scope.words()) != Some(self.scope)
            || (matches!(self.operations, Operations::None(_) | Operations::Sign(_))
                && self.modifiable != Modifiable::No)
        {
            return false;
        }
        match (self.operations, self.scope) {
            (
                Operations::None(_) | Operations::Clone(_) | Operations::Sign(_),
                PendScope::Object,
            )
            | (Operations::Share(_) | Operations::Flat(_), PendScope::Pages(_)) => true,
            (Operations::Nest(rights), PendScope::Nest(NestScope::Whole)) => {
                NestRights::whole().contains(rights)
            }
            (Operations::Nest(rights), PendScope::Nest(NestScope::Reservation { .. })) => {
                NestRights::reservation().contains(rights)
            }
            _ => false,
        }
    }
    /// Checks attenuation and scope identity, not relative range bounds. The
    /// server resolves ranges within the parent's effective window at commit.
    pub fn permits_child(self, child: Self) -> bool {
        self.permits_narrowing(child) && self.distributable == Distributable::Yes
    }
    /// Whole and Reservation scopes, or different ReservationIds, never attenuate into
    /// one another. Relative range bounds must also be checked by the server.
    pub fn permits_narrowing(self, child: Self) -> bool {
        self.valid()
            && child.valid()
            && self.scope.same_kind(child.scope)
            && self.operations.contains(child.operations)
            && self.modifiable.allows(child.modifiable)
            && (self.distributable == Distributable::Yes
                || child.distributable == Distributable::No)
    }
}
