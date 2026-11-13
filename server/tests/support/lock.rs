//! Host synchronization for imported Page transition tests; target interrupt
//! masking and lock ordering are verified by the real-kernel guest suite.
pub(crate) struct SpinLock<T>(std::sync::Mutex<T>);
#[allow(dead_code)]
pub(crate) enum Level {
    L3,
}
impl<T> SpinLock<T> {
    #[allow(dead_code)]
    pub(crate) const fn new_level(_: Level, value: T) -> Self {
        Self::new(value)
    }
    pub(crate) const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
