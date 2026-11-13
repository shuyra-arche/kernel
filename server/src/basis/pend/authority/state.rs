//! Pure borrow decisions. Hardware withdrawal must complete before Borrow or Settle
//! is committed; failure retains the previous obligation and never resumes a parent.
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::pend::{
    CreationRights, Distributable, Limits, Operations, PendRange, PendScope,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Active,
    Retiring,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) access: Modifiable,
    pub(crate) operations: Operations,
    pub(crate) scope: PendScope,
    pub(crate) distributable: Distributable,
    pub(crate) phase: Phase,
    readers: usize,
    writer: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Borrow(Modifiable),
    Return(Modifiable),
    Narrow(Modifiable),
    Limit {
        access: Modifiable,
        operations: Operations,
        scope: PendScope,
        distributable: Distributable,
    },
    Retire,
    Settle,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Denied,
    Busy,
    Dead,
}
impl State {
    pub(crate) const fn new(access: Modifiable) -> Self {
        Self {
            access,
            operations: Operations::None(CreationRights::empty()),
            scope: PendScope::Object,
            distributable: Distributable::No,
            phase: Phase::Active,
            readers: 0,
            writer: false,
        }
    }
    pub(crate) fn authorized(
        access: Modifiable,
        scope: PendScope,
        operations: Operations,
        distributable: Distributable,
    ) -> Self {
        Self {
            operations,
            scope,
            distributable,
            ..Self::new(access)
        }
    }
    pub(crate) fn limits(self) -> Limits {
        Limits {
            operations: self.operations,
            modifiable: self.access,
            distributable: self.distributable,
            scope: self.scope,
        }
    }
    fn contains_scope(self, scope: PendScope) -> bool {
        match (self.scope.range(), scope.range()) {
            (None, None) | (Some(PendRange::Inherit), Some(PendRange::Inherit)) => true,
            (
                Some(PendRange::Pages { offset, count }),
                Some(PendRange::Pages {
                    offset: next_offset,
                    count: next_count,
                }),
            ) => offset.checked_add(count).is_some_and(|end| {
                offset <= next_offset
                    && next_offset
                        .checked_add(next_count)
                        .is_some_and(|next_end| next_end <= end)
            }),
            _ => false,
        }
    }
    pub(crate) fn permits(self, operations: Operations) -> bool {
        self.usable() && self.operations.contains(operations)
    }
    /// An absolute page selection checks scope identity, bounds and operation
    /// rights from one state. Inherit is a derivation request, never a selection.
    pub(crate) fn selects(self, operations: Operations, scope: PendScope) -> bool {
        let requested = Limits {
            operations,
            scope,
            ..self.limits()
        };
        matches!(scope.range(), Some(PendRange::Pages { .. }))
            && self.permits(operations)
            && (self.limits().permits_narrowing(requested)
                || kernel_api::abi::memory::rights::selects(self.limits(), requested))
            && self.contains_scope(scope)
    }
    pub(crate) fn usable(self) -> bool {
        self.phase == Phase::Active
            && !self.writer
            && (self.access == Modifiable::No || self.readers == 0)
    }
    pub(crate) fn transition(self, event: Event) -> Result<Self, Failure> {
        let mut next = self;
        match event {
            Event::Retire => {
                if next.phase != Phase::Dead {
                    next.phase = Phase::Retiring;
                }
            }
            Event::Settle | Event::Cancel => {
                match (event, next.phase) {
                    (Event::Settle, Phase::Retiring | Phase::Dead)
                    | (Event::Cancel, Phase::Active) => {}
                    _ => return Err(Failure::Denied),
                }
                if next.readers != 0 || next.writer {
                    return Err(Failure::Busy);
                }
                next.phase = Phase::Dead;
            }
            Event::Return(access) => match access {
                Modifiable::Yes if next.writer => next.writer = false,
                Modifiable::No if next.readers != 0 => next.readers -= 1,
                _ => return Err(Failure::Denied),
            },
            Event::Borrow(access) => {
                if next.phase != Phase::Active {
                    return Err(Failure::Dead);
                }
                if !next.access.allows(access) {
                    return Err(Failure::Denied);
                }
                if next.writer || (access == Modifiable::Yes && next.readers != 0) {
                    return Err(Failure::Busy);
                }
                match access {
                    Modifiable::Yes => next.writer = true,
                    Modifiable::No => {
                        next.readers = next.readers.checked_add(1).ok_or(Failure::Busy)?
                    }
                }
            }
            Event::Limit {
                access,
                operations,
                scope,
                distributable,
            } => {
                let limits = Limits {
                    operations,
                    modifiable: access,
                    distributable,
                    scope,
                };
                if !next.limits().permits_narrowing(limits) || !next.contains_scope(scope) {
                    return Err(Failure::Denied);
                }
                next = next.transition(Event::Narrow(access))?;
                next.operations = operations;
                next.scope = scope;
                next.distributable = distributable;
            }
            Event::Narrow(access) => {
                if next.phase != Phase::Active {
                    return Err(Failure::Dead);
                }
                if !next.access.allows(access) {
                    return Err(Failure::Denied);
                }
                if next.readers != 0 || next.writer {
                    return Err(Failure::Busy);
                }
                next.access = access;
            }
        }
        Ok(next)
    }
}
