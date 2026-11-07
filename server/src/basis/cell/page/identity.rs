use core::num::NonZeroUsize;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

/// 启动期内不复用的物理实例号；PA 归还后再分配也不能复活旧身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PageId(NonZeroUsize);

static NEXT: AtomicUsize = AtomicUsize::new(1);

impl PageId {
    pub(super) fn allocate() -> Option<Self> {
        // 只分配身份，不发布 Page 内容；可见性由外层持有/提交同步提供。
        // 耗尽后拒绝，绝不 wrap 回旧实例。失败构造消耗的号也不再使用。
        let mut next = NEXT.load(Ordering::Relaxed);
        loop {
            let after = next.checked_add(1)?;
            match NEXT.compare_exchange_weak(next, after, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return NonZeroUsize::new(next).map(Self),
                Err(observed) => next = observed,
            }
        }
    }
}
