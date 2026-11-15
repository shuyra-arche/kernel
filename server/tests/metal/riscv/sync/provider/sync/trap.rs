use std::cell::Cell;
std::thread_local! {static ENABLED:Cell<bool>=const {Cell::new(true)};}
pub(crate) fn enabled() -> bool {
    ENABLED.get()
}
std::thread_local! {
    static RESTORE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn on_restore(callback: impl FnOnce() + 'static) {
    RESTORE.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Box::new(callback));
    });
}
pub(crate) struct TrapGuard(bool, core::marker::PhantomData<*const ()>);
impl TrapGuard {
    pub(crate) unsafe fn save() -> Self {
        Self(ENABLED.replace(false), core::marker::PhantomData)
    }
}
impl Drop for TrapGuard {
    fn drop(&mut self) {
        if self.0 {
            let callback = RESTORE.with_borrow_mut(Option::take);
            if let Some(callback) = callback {
                callback();
            }
            ENABLED.set(true);
        }
    }
}
