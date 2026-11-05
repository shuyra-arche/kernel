/// Unique identity of an installed TLS slot. State remains in the TLS table.
///
/// This token must be retired by its current task before the terminal trap.
/// Dropping it has no effects; runtimes must keep it until explicit retirement.
pub struct Tls {
    index: usize,
    task_local: core::marker::PhantomData<*mut ()>,
}

impl Tls {
    pub(super) fn new(index: usize) -> Self {
        Self {
            index,
            task_local: core::marker::PhantomData,
        }
    }
    pub(super) fn index(&self) -> usize {
        self.index
    }
}
