//! Per-recipient CPU leaf observations. NotObserved is never global cleanliness,
//! device-write exclusion, persistence or permission to reclaim a Page.
use crate::{
    abi::layout::{fetch_words, store_words},
    model::nest::{MemoGeneration, MemoPosition},
};
use core::sync::atomic::{AtomicU64, Ordering};
pub const VERSION: u64 = 1;
pub const MAX_SUBSCRIPTIONS: usize = 16;
pub const MIN_PERIOD_NS: u64 = 10_000_000;
pub const MAX_PERIOD_NS: u64 = 10_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub position: MemoPosition,
    pub generation: MemoGeneration,
    pub period_ns: u64,
}
impl Request {
    pub const WIDTH: usize = 56;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut b = [0; Self::WIDTH];
        b[..32].copy_from_slice(&self.position.to_bytes());
        b[32..40].copy_from_slice(&self.generation.to_bytes());
        b[40..48].copy_from_slice(&self.period_ns.to_le_bytes());
        b[48..].copy_from_slice(&VERSION.to_le_bytes());
        b
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() != Self::WIDTH || u64::from_le_bytes(b[48..].try_into().ok()?) != VERSION {
            return None;
        }
        let r = Self {
            position: MemoPosition::from_bytes(&b[..32])?,
            generation: MemoGeneration::from_bytes(&b[32..40])?,
            period_ns: u64::from_le_bytes(b[40..48].try_into().ok()?),
        };
        (MIN_PERIOD_NS..=MAX_PERIOD_NS)
            .contains(&r.period_ns)
            .then_some(r)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Validity {
    Unknown = 0,
    Pending = 1,
    Observed = 2,
    Stale = 3,
    Unsupported = 4,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Evidence {
    Unknown = 0,
    NotObserved = 1,
    Observed = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub sequence: u64,
    pub epoch: u64,
    pub nest: u64,
    pub reservation: u64,
    pub position: u64,
    pub generation: u64,
    pub page: u64,
    pub address: u64,
    pub bytes: u64,
    pub start_ns: u64,
    pub end_ns: u64,
    pub synchronized_ns: u64,
    pub validity: Validity,
    pub accessed: Evidence,
    pub dirty: Evidence,
}
const WORDS: usize = 16;
impl Sample {
    pub const WIDTH: usize = 128;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut b = [0; Self::WIDTH];
        store_words(
            [
                VERSION as usize,
                self.sequence as usize,
                self.epoch as usize,
                self.nest as usize,
                self.reservation as usize,
                self.position as usize,
                self.generation as usize,
                self.page as usize,
                self.address as usize,
                self.bytes as usize,
                self.start_ns as usize,
                self.end_ns as usize,
                self.synchronized_ns as usize,
                self.validity as usize,
                self.accessed as usize,
                self.dirty as usize,
            ],
            &mut b,
        );
        b
    }
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let w: [usize; WORDS] = fetch_words(b)?;
        if w[0] != VERSION as usize
            || w[1] == 0
            || w[3] == 0
            || w[4] == 0
            || w[6] == 0
            || w[7] == 0
            || [w[3], w[4], w[7]]
                .iter()
                .any(|id| *id > isize::MAX as usize)
            || w[5].checked_add(1).is_none()
            || w[8].checked_add(4096).is_none()
            || w[9] != 4096
            || !w[8].is_multiple_of(4096)
            || w[10] > w[11]
            || w[11] > w[12]
        {
            return None;
        }
        let validity = match w[13] {
            0 => Validity::Unknown,
            1 => Validity::Pending,
            2 => Validity::Observed,
            3 => Validity::Stale,
            4 => Validity::Unsupported,
            _ => return None,
        };
        fn evidence(n: usize) -> Option<Evidence> {
            Some(match n {
                0 => Evidence::Unknown,
                1 => Evidence::NotObserved,
                2 => Evidence::Observed,
                _ => return None,
            })
        }
        let accessed = evidence(w[14])?;
        let dirty = evidence(w[15])?;
        if if validity == Validity::Observed {
            w[2] == 0 || accessed == Evidence::Unknown || dirty == Evidence::Unknown
        } else {
            accessed != Evidence::Unknown || dirty != Evidence::Unknown
        } {
            return None;
        }
        Some(Self {
            sequence: w[1] as u64,
            epoch: w[2] as u64,
            nest: w[3] as u64,
            reservation: w[4] as u64,
            position: w[5] as u64,
            generation: w[6] as u64,
            page: w[7] as u64,
            address: w[8] as u64,
            bytes: w[9] as u64,
            start_ns: w[10] as u64,
            end_ns: w[11] as u64,
            synchronized_ns: w[12] as u64,
            validity,
            accessed,
            dirty,
        })
    }
}
/// One producer. Consume the paired Sign only after a successful coherent read.
/// That exact acknowledgement permits replacing the page; unread facts remain
/// in Memo history and the recipient's cursor until a later acknowledgement.
#[repr(C, align(4096))]
pub struct SamplePage {
    sequence: AtomicU64,
    words: [AtomicU64; WORDS],
    reserved: [u64; 512 - WORDS - 1],
}
impl Default for SamplePage {
    fn default() -> Self {
        Self::new()
    }
}
impl SamplePage {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            words: [const { AtomicU64::new(0) }; WORDS],
            reserved: [0; 512 - WORDS - 1],
        }
    }
    pub fn publish(&self, s: Sample) -> bool {
        let Some(next) = self.sequence.load(Ordering::SeqCst).checked_add(2) else {
            return false;
        };
        self.sequence.store(next - 1, Ordering::SeqCst);
        let b = s.to_bytes();
        for (target, word) in self.words.iter().zip(b.chunks_exact(8)) {
            target.store(
                u64::from_le_bytes(word.try_into().unwrap()),
                Ordering::SeqCst,
            );
        }
        self.sequence.store(next, Ordering::SeqCst);
        true
    }
    pub fn read(&self) -> Option<Sample> {
        let first = self.sequence.load(Ordering::SeqCst);
        if first == 0 || first & 1 != 0 {
            return None;
        }
        let mut b = [0; Sample::WIDTH];
        for (word, out) in self.words.iter().zip(b.chunks_exact_mut(8)) {
            out.copy_from_slice(&word.load(Ordering::SeqCst).to_le_bytes());
        }
        if self.sequence.load(Ordering::SeqCst) != first {
            return None;
        }
        Sample::from_bytes(&b)
    }
}
impl crate::abi::reply::FromPair
    for (
        crate::model::identity::PendId,
        crate::model::identity::PendId,
    )
{
    fn from_pair(a: usize, b: usize) -> Self {
        assert!(
            a > 0 && a <= isize::MAX as usize && b > 0 && b <= isize::MAX as usize && a != b,
            "invalid activity grants"
        );
        (
            crate::model::identity::PendId::mint(a),
            crate::model::identity::PendId::mint(b),
        )
    }
}
