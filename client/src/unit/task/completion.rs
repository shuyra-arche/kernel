use super::*;

pub struct Completion<T> {
    pub(super) state: AtomicUsize,
    pub(super) result: Option<T>,
}

impl<T> Completion<T> {
    pub(super) const fn new() -> Self {
        Self {
            state: AtomicUsize::new(0),
            result: None,
        }
    }
}
