/// A state revision, independent of the identity of its owning object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Generation(u64);

impl Generation {
    pub(crate) const fn first() -> Self {
        Self(1)
    }
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
    pub(crate) fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}
