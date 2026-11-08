/// CPU invalidation needed by a batch of translation edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    AddressSpace,
    Global,
}

impl Scope {
    pub(crate) fn include(self, global: bool) -> Self {
        if global { Self::Global } else { self }
    }
}

/// # Safety
/// Success must cover local and remote CPUs that may still use the old
/// translations. Order the page-table edits and CPU address-space leases
/// against invalidation. Partial completion or error cannot attest success.
pub(crate) unsafe trait Shootdown {
    type Target;
    type Error;

    fn invalidate(&self, target: &Self::Target) -> Result<(), Self::Error>;
}

/// The backend confirmed completion for this invocation and target.
#[must_use]
pub(crate) struct Completed<T> {
    target: T,
}

impl<T> Completed<T> {
    pub(crate) fn target(self) -> T {
        self.target
    }
}

pub(crate) fn invalidate<B: Shootdown>(
    backend: &B,
    target: B::Target,
) -> Result<Completed<B::Target>, B::Error> {
    backend.invalidate(&target)?;
    Ok(Completed { target })
}
