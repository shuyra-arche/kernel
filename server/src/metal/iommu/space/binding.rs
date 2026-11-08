use super::Projection;
use crate::basis::cell::page::Retention;

use super::Phase;

pub(super) struct Binding {
    pub(super) projection: Projection,
    pub(super) retention: Option<Retention>,
    pub(super) phase: Phase,
}

impl Binding {
    pub(super) fn begin_retirement(&mut self) {
        let retained = self.retention.take().expect("one retention per binding");
        self.retention = Some(retained.retire());
    }
}
