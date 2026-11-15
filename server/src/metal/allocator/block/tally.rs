use kernel_api::model::cell::PAGE_SIZE;

use super::metadata::BlockMeta;

pub(super) struct Tally {
    pages: crate::metal::allocator::pages::Pages<'static>,
    cells: *mut BlockMeta,
    len: usize,
    guard: crate::basis::sync::LegacyLock<crate::basis::sync::order::Tally, ()>,
}

// SAFETY: `cells` 指向 tally 自己的簿记表，所有读写都经过 `guard` 的原生锁。
unsafe impl Sync for Tally {}

impl Tally {
    pub(super) fn new(
        pages: crate::metal::allocator::pages::Pages<'static>,
        cells: *mut BlockMeta,
        len: usize,
    ) -> Self {
        Self {
            pages,
            cells,
            len,
            guard: crate::basis::sync::LegacyLock::new(()),
        }
    }

    fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        self.guard.with(|()| f())
    }

    fn idx(&self, pa: usize) -> Option<usize> {
        let page = pa & !(PAGE_SIZE - 1);
        let idx = self.pages.index(page)?;
        (idx < self.len).then_some(idx)
    }

    pub(super) fn frame_of(&self, idx: usize) -> usize {
        self.pages.address(idx).expect("valid compact Tally index")
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn owner_of(&self, pa: usize) -> Option<usize> {
        self.with(|| {
            let m = unsafe { self.cells.add(self.idx(pa)?).read() };
            m.owner()
        })
    }

    pub(super) fn read_idx(&self, idx: usize) -> BlockMeta {
        self.with(|| unsafe { self.cells.add(idx).read() })
    }

    pub(super) fn write(&self, page: usize, m: BlockMeta) {
        self.with(|| {
            let idx = self.idx(page).expect("block tally: page out of table");
            unsafe {
                self.cells.add(idx).write(m);
            }
        })
    }

    pub(super) fn inc_used(&self, page: usize) {
        self.with(|| {
            let idx = self.idx(page).expect("block tally: page out of table");
            unsafe {
                let m = self.cells.add(idx).read().inc_used();
                self.cells.add(idx).write(m);
            }
        })
    }

    pub(super) fn dec_used(&self, page: usize) -> bool {
        self.with(|| {
            let idx = self.idx(page).expect("block tally: page out of table");
            unsafe {
                let (m, empty) = self.cells.add(idx).read().dec_used();
                self.cells.add(idx).write(m);
                empty
            }
        })
    }
}
