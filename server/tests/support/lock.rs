//! Host synchronization for imported Page transition tests; target interrupt
//! masking and lock ordering are verified by the real-kernel guest suite.
pub(crate) struct SpinLock<T>(std::sync::Mutex<T>);
impl<T> SpinLock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
