use ::core::fmt;
use kernel_api::abi::call::pend::PendFail;

pub(crate) struct Failure<T> {
    pub reason: PendFail,
    pub value: T,
}

impl<T> Failure<T> {
    pub(crate) fn into_parts(self) -> (PendFail, T) {
        (self.reason, self.value)
    }
}

impl<T> fmt::Debug for Failure<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(f)
    }
}
