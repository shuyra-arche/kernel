use core::num::NonZeroUsize;

/// 启动期内不复用的物理实例号；PA 归还后再分配也不能复活旧身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PageId(NonZeroUsize);

static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);

impl PageId {
    pub(crate) fn get(self) -> usize {
        self.0.get()
    }
    pub(super) fn allocate() -> Option<Self> {
        NEXT.allocate().and_then(NonZeroUsize::new).map(Self)
    }
}
