use super::*;

#[derive(Clone, Copy)]
pub(super) struct SendSlot<T>(pub(super) *mut Completion<T>);

unsafe impl<T> Send for SendSlot<T> {}
unsafe impl<T> Sync for SendSlot<T> {}

impl<T> SendSlot<T> {
    /// Publish T before DONE. The parent or detached-slot registry owns freeing.
    pub(super) unsafe fn store_result(self, r: T) {
        unsafe {
            (*self.0).result = Some(r);
            (*self.0).state.fetch_or(DONE, Ordering::Release);
        }
    }
}
