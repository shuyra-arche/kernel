//! The one statically configured interrupt-window service, not a callback registry.
use crate::metal::generic::sync::{Environment, once::OnceLock};
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BindingError {
    Missing,
    Duplicate,
}
pub(crate) struct Service<E: Environment>(OnceLock<fn(usize), E>);
impl<E: Environment> Service<E> {
    pub(crate) const fn new() -> Self {
        Self(OnceLock::new())
    }
    pub(crate) fn bind(&self, service: fn(usize)) -> Result<(), BindingError> {
        self.0.set(service).map_err(|_| BindingError::Duplicate)
    }
    pub(crate) fn require(&self) -> Result<fn(usize), BindingError> {
        self.0.get().copied().ok_or(BindingError::Missing)
    }
}
