/// Unique identity of an installed TLS slot. State remains in the TLS table.
///
/// Keep this task-local token for explicit retirement, or retain the Caller
/// mapping through the existing Kernel terminal path. The table remains
/// responsible until explicit cleanup or native terminal observation; dropping
/// the token has no effect on tp, mappings, or that responsibility. A retained
/// Live slot is not guaranteed to be swept before the address space ends.
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
