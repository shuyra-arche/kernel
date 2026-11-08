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

/// Exact software address-domain instance, attempt, hardware identity and coverage.
/// The full address space is the minimum CPU scope; global leaves require Global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Request<I> {
    pub(crate) domain: usize,
    pub(crate) attempt: u64,
    pub(crate) identifier: I,
    pub(crate) scope: Scope,
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

impl<T: PartialEq> Completed<T> {
    pub(crate) fn confirms(self, expected: T) -> bool {
        self.target == expected
    }
}

pub(crate) fn invalidate<B: Shootdown>(
    backend: &B,
    target: B::Target,
) -> Result<Completed<B::Target>, B::Error> {
    backend.invalidate(&target)?;
    Ok(Completed { target })
}
