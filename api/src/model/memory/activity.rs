//! Per-recipient CPU leaf observations. NotObserved is never global cleanliness,
//! device-write exclusion, persistence or permission to reclaim a Page.
use crate::abi::layout::{fetch_words, store_words};
use core::sync::atomic::{AtomicU64, Ordering};
pub const VERSION: u64 = 1;
pub const MAX_SUBSCRIPTIONS: usize = 16;
/// Receiver-local (Sign Pend, Share View Pend). The Sign's descriptive Mark is
/// the Share Pend's non-reusing ID, not a new subscription authority/handle.
pub type Pair = (
    crate::model::identity::PendId,
    crate::model::identity::PendId,
);
/// Initial provider policy. Clients can aggregate intervals in user space.
pub const PERIOD_NS: u64 = 10_000_000;
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
    pub span: u64,
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
                self.span as usize,
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
            span: w[4] as u64,
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

/// Readonly View limits, using the observed Span's fixed-origin page.
/// Only that coordinate is backed in the new View; no adjacent facts exist.
pub fn limits(page: usize) -> Option<crate::model::pend::Limits> {
    use crate::model::{modifiable::Modifiable, pend::*};
    page.checked_add(1)?.checked_mul(4096)?;
    Some(Limits {
        operations: Operations::Share(ShareRights::ASSOCIATE),
        modifiable: Modifiable::No,
        distributable: Distributable::No,
        scope: PendScope::Pages(PendRange::Pages {
            offset: page,
            count: 1,
        }),
    })
}
/// Check a collected pair's Kernel-origin metadata and least-privilege shape.
/// Marks correlate the two Pends; live authority is still checked by each call.
pub fn pair(
    sign: crate::model::pend::PendInfo,
    view: crate::model::pend::PendInfo,
) -> Option<Pair> {
    use crate::model::{modifiable::Modifiable, pend::*, sign::SignRights};
    if sign.token.get() == 0
        || view.token.get() == 0
        || !sign.limits.valid()
        || !view.limits.valid()
        || sign.token == view.token
        || sign.owner.get() != 0
        || view.owner.get() != 0
        || sign.lender.get() != 0
        || view.lender.get() != 0
        || sign.status != PendStatus::Active
        || view.status != PendStatus::Active
        || sign.mark.get() != view.token.get() as u64
        || sign.limits.modifiable != Modifiable::No
        || view.limits.modifiable != Modifiable::No
        || sign.limits.distributable != Distributable::No
        || view.limits.distributable != Distributable::No
        || !matches!(sign.target, Target::Sign(_))
        || !matches!(view.target, Target::View(ViewTarget::Share(_)))
        || sign.limits.operations != Operations::Sign(SignRights::LISTEN | SignRights::CONSUME)
        || sign.limits.scope != PendScope::Object
        || view.limits.operations != Operations::Share(ShareRights::ASSOCIATE)
        || !matches!(
            view.limits.scope,
            PendScope::Pages(PendRange::Pages { count: 1, .. })
        )
    {
        return None;
    }
    Some((sign.token, view.token))
}
