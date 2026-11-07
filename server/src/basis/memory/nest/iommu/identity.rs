use core::num::{NonZeroU32, NonZeroUsize};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use super::Error;

static NEXT_BINDING: AtomicUsize = AtomicUsize::new(1);
static NEXT_BATCH: AtomicU32 = AtomicU32::new(1);

/// 跨 IoSpace 不复用，错误地址空间上的句柄不会误中同号记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingId(NonZeroUsize);

impl BindingId {
    pub(super) fn allocate() -> Result<Self, Error> {
        let mut before = NEXT_BINDING.load(Ordering::Relaxed);
        loop {
            let after = before.checked_add(1).ok_or(Error::Exhausted)?;
            match NEXT_BINDING.compare_exchange_weak(
                before,
                after,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Ok(Self(
                        NonZeroUsize::new(before).expect("identities start at one"),
                    ));
                }
                Err(current) => before = current,
            }
        }
    }
}

/// 启动期内不复用的撤销批次；也可作为 IOFENCE.C 的 32 位完成值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BatchId(NonZeroU32);

impl BatchId {
    pub(super) fn allocate() -> Result<Self, Error> {
        let mut before = NEXT_BATCH.load(Ordering::Relaxed);
        loop {
            let after = before.checked_add(1).ok_or(Error::Exhausted)?;
            match NEXT_BATCH.compare_exchange_weak(
                before,
                after,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(Self(NonZeroU32::new(before).expect("batches start at one"))),
                Err(current) => before = current,
            }
        }
    }

    pub(crate) fn value(self) -> u32 {
        self.0.get()
    }
}
