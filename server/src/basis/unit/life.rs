use alloc::sync::Arc;
use alloc::sync::Weak;

pub(crate) use super::team::life::TeamLife;

#[derive(Debug)]
pub(crate) struct Life;

impl Life {
    pub(crate) fn try_new() -> Result<Arc<Life>, crate::basis::nest::NestError> {
        Arc::try_new_in(Life, alloc::alloc::Global)
            .map_err(|_| crate::basis::nest::NestError::OutOfMemory)
    }

    pub(crate) fn live(w: &Weak<Life>) -> bool {
        w.strong_count() > 0
    }

    pub(crate) fn dead(w: &Weak<Life>) -> bool {
        !Self::live(w)
    }
}
