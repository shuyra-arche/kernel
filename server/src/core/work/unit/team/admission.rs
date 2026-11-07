//! Membership admission is independent of whether existing executions may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    Open,
    Closed,
}
impl Admission {
    pub(crate) fn finish(self) -> Self {
        Self::Closed
    }
    pub(crate) fn permits(self) -> bool {
        self == Self::Open
    }
}
