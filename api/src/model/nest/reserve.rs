//! Source-free reservation geometry. Admission and active Sv mode are checked by Kernel.

use crate::{
    abi::layout::{fetch_words, store_words},
    address::{BASE_PAGE_SIZE, PageCount, VirtAddr},
    model::{
        cell::{CellLevel, Placement},
        identity::PendId,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveRequest {
    pub placement: Placement,
    pub count: PageCount,
    pub level: CellLevel,
    pub access: u64,
}

impl ReserveRequest {
    pub const WIDTH: usize = 48;

    pub fn valid(self) -> bool {
        self.count.checked_bytes().is_some()
            && self
                .level
                .pages()
                .is_some_and(|pages| self.count.get().is_multiple_of(pages))
            && self.access & !14 == 0
            && self.access & 10 != 0
            && (self.access & 4 == 0 || self.access & 2 != 0)
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let [tag, address, align] = self.placement.encode();
        let mut bytes = [0; Self::WIDTH];
        store_words(
            [
                tag,
                address,
                align,
                self.count.get(),
                self.level.get(),
                self.access as usize,
            ],
            &mut bytes,
        );
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [tag, address, align, count, level, access] = fetch_words(bytes)?;
        let request = Self {
            placement: Placement::decode([tag, address, align]).ok()?,
            count: PageCount::new(count)?,
            level: CellLevel::new(level)?,
            access: access as u64,
        };
        request.valid().then_some(request)
    }
}

impl crate::abi::reply::FromPair for (PendId, VirtAddr) {
    fn from_pair(token: usize, address: usize) -> Self {
        assert!(
            token != 0 && token <= isize::MAX as usize,
            "invalid reservation Pend"
        );
        assert!(
            address.is_multiple_of(BASE_PAGE_SIZE),
            "invalid reservation address"
        );
        (PendId::new(token), VirtAddr::new(address))
    }
}
