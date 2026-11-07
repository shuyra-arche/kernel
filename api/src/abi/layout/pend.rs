use crate::model::{
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    pend::{PendInfo, PendStatus},
    view::{ViewId, ViewKind},
};
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
