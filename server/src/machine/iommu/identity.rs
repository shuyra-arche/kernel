use core::num::NonZeroU32;
use core::num::NonZeroUsize;

use super::Error;

static NEXT_BINDING: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);
static NEXT_BATCH: crate::basis::name::Serial =
    crate::basis::name::Serial::new(1, u32::MAX as usize);

/// 跨 IoSpace 不复用，错误地址空间上的句柄不会误中同号记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingId(NonZeroUsize);

impl BindingId {
    pub(super) fn allocate() -> Result<Self, Error> {
        NEXT_BINDING
            .allocate()
            .and_then(NonZeroUsize::new)
            .map(Self)
            .ok_or(Error::Exhausted)
    }
}

/// 启动期内不复用的撤销批次；也可作为 IOFENCE.C 的 32 位完成值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BatchId(NonZeroU32);

impl BatchId {
    pub(super) fn allocate() -> Result<Self, Error> {
        NEXT_BATCH
            .allocate()
            .and_then(|n| NonZeroU32::new(n as u32))
            .map(Self)
            .ok_or(Error::Exhausted)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.get()
    }
}
