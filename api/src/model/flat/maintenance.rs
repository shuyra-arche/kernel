//! Maintenance record identity for the next memory ABI, not an authorization
//! object. The Flat owns the record, its branch/actor binding and its lifecycle.

use super::FlatId;
use core::num::NonZeroU64;

/// Non-wrapping generation of one maintenance record. Takeover advances it;
/// an old generation never becomes usable again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockGeneration(NonZeroU64);

impl BlockGeneration {
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Option<Self> {
        Self::new(self.get().checked_add(1)?)
    }
}

/// Locates one Flat maintenance record. A use must additionally supply a live
/// Flat Pend; the server checks its exact branch, Task, range and operation bits.
/// Record IDs are never reused within a Flat. This is not accepted by ABI v10.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRef {
    flat: FlatId,
    record: NonZeroU64,
    generation: BlockGeneration,
}

impl BlockRef {
    pub const WIDTH: usize = 24;

    pub const fn new(flat: FlatId, record: u64, generation: BlockGeneration) -> Option<Self> {
        match NonZeroU64::new(record) {
            Some(record) => Some(Self {
                flat,
                record,
                generation,
            }),
            None => None,
        }
    }

    pub const fn flat(self) -> FlatId {
        self.flat
    }

    pub const fn record(self) -> u64 {
        self.record.get()
    }

    pub const fn generation(self) -> BlockGeneration {
        self.generation
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        for (word, dest) in [self.flat.get() as u64, self.record(), self.generation.get()]
            .into_iter()
            .zip(bytes.chunks_exact_mut(8))
        {
            dest.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::WIDTH] = bytes.try_into().ok()?;
        let flat = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        let record = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let generation = u64::from_le_bytes(bytes[16..].try_into().ok()?);
        Self::new(
            FlatId::new(usize::try_from(flat).ok()?)?,
            record,
            BlockGeneration::new(generation)?,
        )
    }
}
