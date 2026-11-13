//! A holding transfer carries restrictions, never a replacement permission.
//! The live Pend remains authoritative; the immutable ceiling only narrows it.
use super::*;
use kernel_api::model::{identity::TaskId, pend::FlatRights};

pub(crate) struct Carried {
    authority: Arc<Pend>,
    actor: TaskId,
    ceiling: Limits,
    offset: PageOffset,
    count: PageCount,
}
impl Carried {
    pub(crate) fn new(
        authority: &Arc<Pend>,
        actor: TaskId,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<Self, PendFail> {
        if !authority.selects(
            Operations::Flat(FlatRights::TRANSFER),
            PendScope::Pages(PendRange::Pages {
                offset: offset.get(),
                count: count.get(),
            }),
        ) {
            return Err(PendFail::Denied);
        }
        Ok(Self {
            authority: authority.clone(),
            actor,
            ceiling: authority.limits(),
            offset,
            count,
        })
    }
    pub(crate) fn authority(&self) -> &Arc<Pend> {
        &self.authority
    }
    pub(crate) fn live(&self) -> bool {
        self.authority.alive() && self.authority.contains(self.offset, self.count)
    }
    pub(crate) fn returns_to(
        &self,
        authority: &Pend,
        offset: PageOffset,
        count: PageCount,
    ) -> bool {
        self.offset == offset
            && self.count == count
            && authority.permits(Operations::Flat(FlatRights::TRANSFER))
            && authority.contains(offset, count)
            && self.authority.descends_from(authority)
    }
    pub(crate) fn permits(
        &self,
        actor: TaskId,
        rights: FlatRights,
        access: Option<Modifiable>,
        export: bool,
    ) -> bool {
        let current = self.authority.limits();
        self.live()
            && self.ceiling.operations.contains(Operations::Flat(rights))
            && self.authority.permits(Operations::Flat(rights))
            && access.is_none_or(|need| {
                self.ceiling.modifiable.allows(need)
                    && self
                        .authority
                        .modifiable()
                        .is_some_and(|mode| mode.allows(need))
            })
            && (!(export || actor != self.actor)
                || self.ceiling.distributable == Distributable::Yes
                    && current.distributable == Distributable::Yes)
    }
    /// Recovery is called only after the owning Block settles old publications.
    /// Neither sibling grants nor stronger ancestors can widen the saved ceiling.
    pub(crate) fn recover(&mut self, actor: TaskId, authority: &Arc<Pend>) {
        if !self.live()
            && authority.alive()
            && self.authority.descends_from(authority)
            && authority.contains(self.offset, self.count)
        {
            let old = self.authority.limits();
            if old.modifiable == Modifiable::No {
                self.ceiling.modifiable = Modifiable::No;
            }
            if old.distributable == Distributable::No {
                self.ceiling.distributable = Distributable::No;
            }
            if let (Operations::Flat(saved), Operations::Flat(current)) =
                (self.ceiling.operations, old.operations)
            {
                self.ceiling.operations = Operations::Flat(saved & current);
            }
            self.authority = authority.clone();
            self.actor = actor;
        }
    }
}
