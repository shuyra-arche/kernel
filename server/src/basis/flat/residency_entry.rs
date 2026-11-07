use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;

pub(super) struct ResidencyEntry {
    pub id: usize,
    pub index: PageIndex,
    pub count: PageCount,
}
