use alloc::sync::Arc;
use alloc::sync::Weak;

#[derive(Debug)]
pub(crate) struct Anchor;

impl Anchor {
    pub(crate) fn try_new() -> Result<Arc<Anchor>, crate::basis::nest::NestError> {
        Arc::try_new_in(Anchor, alloc::alloc::Global)
            .map_err(|_| crate::basis::nest::NestError::OutOfMemory)
    }

    pub(crate) fn live(w: &Weak<Anchor>) -> bool {
        w.strong_count() > 0
    }

    pub(crate) fn dead(w: &Weak<Anchor>) -> bool {
        !Self::live(w)
    }
}
