use crate::basis::memory::address::{PageCount, PageIndex};

pub(super) struct ResidencyEntry {
    pub id: usize,
    pub index: PageIndex,
    pub count: PageCount,
}
