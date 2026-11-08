use crate::basis::name::Recycle;
use crate::machine::sync::Level;
use crate::machine::sync::SpinLock;

pub(crate) const ASID_BITS: u32 = 16;
pub(super) const VACANT: usize = 1 << ASID_BITS;

const _: () = assert!(ASID_BITS == 16);

pub(crate) const fn vacant() -> usize {
    VACANT
}

static ASID_ALLOCATOR: SpinLock<Recycle> = SpinLock::new_level(Level::Asid, Recycle::new(1, 65536));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Asid(usize);

impl Asid {
    pub(crate) const fn kernel() -> Self {
        Self(0)
    }

    pub(crate) const fn from_raw(raw: usize) -> Self {
        Self(raw & 0xFFFF)
    }

    pub(crate) fn try_allocate() -> Option<Self> {
        ASID_ALLOCATOR.lock().allocate().map(Self)
    }

    /// 归还一个尚未发布的 ASID 号。
    pub(crate) fn release_unpublished(asid: Self) {
        debug_assert!(!asid.is_kernel());
        assert!(
            ASID_ALLOCATOR.lock().release(asid.get()),
            "asid: unpublished allocation must be owned"
        );
    }

    /// 归还一个已经失效完成的 ASID 号。
    pub(crate) fn release(asid: Self) {
        assert!(
            ASID_ALLOCATOR.lock().release(asid.get()),
            "asid: double-free or never-allocated"
        );
    }

    pub fn get(self) -> usize {
        self.0
    }

    pub fn is_kernel(self) -> bool {
        self.0 == 0
    }
}
