use std::cell::Cell;
std::thread_local! {static ENABLED:Cell<bool>=const {Cell::new(true)};}
pub(crate) fn enabled() -> bool {
    ENABLED.get()
}
pub(crate) struct TrapGuard(bool);
impl TrapGuard {
    pub(crate) unsafe fn save() -> Self {
        Self(ENABLED.replace(false))
    }
}
impl Drop for TrapGuard {
    fn drop(&mut self) {
        if self.0 {
            ENABLED.set(true);
        }
    }
}
