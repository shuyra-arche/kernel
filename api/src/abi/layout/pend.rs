use super::{fetch_words, store_words};
use crate::abi::register::Wire;
use crate::model::{
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    pend::{AccordRequest, AccordTarget, PendInfo, PendRange, PendStatus},
    view::{ViewId, ViewKind},
};

impl AccordRequest {
    /// Eight little-endian RV64 words: source, target tag, target ID, access,
    /// mark, range tag, offset, count. Caller has a zero target ID.
    pub const WIDTH: usize = 64;

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let (tag, id) = match self.target {
            AccordTarget::Caller => (0, 0),
            AccordTarget::Task(id) => (1, id.get()),
        };
        let mut bytes = [0; Self::WIDTH];
        store_words(
            [self.src.get(), tag, id, self.subset.bits() as usize],
            &mut bytes[..32],
        );
        bytes[32..40].copy_from_slice(&self.mark.get().to_le_bytes());
        let mut range = [0; 6];
        self.range.pack(&mut range, &mut 0);
        store_words([range[0], range[1], range[2]], &mut bytes[40..]);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let [src, tag, id, subset] = fetch_words(&bytes[..32])?;
        if src == 0 {
            return None;
        }
        let target = match (tag, id) {
            (0, 0) => AccordTarget::Caller,
            (1, id) if id != 0 => AccordTarget::Task(TaskId::new(id)),
            _ => return None,
        };
        let [tag, offset, count] = fetch_words(&bytes[40..])?;
        Some(Self {
            src: PendId::new(src),
            target,
            subset: Modifiable::from_bits(u32::try_from(subset).ok()?)?,
            mark: Mark::new(u64::from_le_bytes(bytes[32..40].try_into().ok()?)),
            range: PendRange::unpack(&[tag, offset, count, 0, 0, 0], &mut 0).ok()?,
        })
    }
}

impl PendInfo {
    pub const WORDS: usize = 8;
    pub const SIZE: usize = Self::WORDS * core::mem::size_of::<usize>();
    pub fn words(self) -> [usize; Self::WORDS] {
        [
            self.token.get(),
            self.view.get(),
            self.kind as usize,
            self.modifiable.bits() as usize,
            self.owner.get(),
            self.lender.get(),
            self.mark.get() as usize,
            self.status as usize,
        ]
    }
    pub fn from_words(w: [usize; Self::WORDS]) -> Option<Self> {
        if w[0] == 0 || w[1] == 0 {
            return None;
        }
        Some(Self {
            token: PendId::new(w[0]),
            view: ViewId::new(w[1]),
            kind: ViewKind::of(w[2])?,
            modifiable: Modifiable::from_bits(u32::try_from(w[3]).ok()?)?,
            owner: TaskId::new(w[4]),
            lender: TaskId::new(w[5]),
            mark: Mark::new(w[6] as u64),
            status: PendStatus::of(w[7])?,
        })
    }
}
