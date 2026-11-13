//! One maintenance owner and non-reusable epoch. Physical settlement is checked
//! by RangeBlock before calling takeover; this record cannot discharge a receipt.
use crate::basis::{
    pend::{self, Pend},
    unit::task::Task,
};
use alloc::sync::{Arc, Weak};
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        flat::{BlockPhase, BlockRef},
        identity::{PendId, TaskId},
        modifiable::Modifiable,
        pend::{FlatRights, Operations},
    },
};

pub(super) struct Epoch {
    pub(super) reference: BlockRef,
    pub(super) actor: TaskId,
    pub(super) task: Weak<Task>,
    pub(super) token: PendId,
    pub(super) authority: Arc<Pend>,
    pub(super) base: usize,
    pub(super) data: Modifiable,
    pub(super) associate: bool,
    pub(super) phase: BlockPhase,
}
// SAFETY: token is a lookup identity paired with its original Task, never a
// handle interpreted in another Task's ledger. RangeBlock locks this record;
// authorization transitions additionally hold COMMIT.
unsafe impl Send for Epoch {}
impl Epoch {
    pub(super) fn new(
        reference: BlockRef,
        actor: &Arc<Task>,
        token: PendId,
        authority: &Arc<Pend>,
        base: usize,
    ) -> Self {
        Self {
            reference,
            actor: actor.ident.id,
            task: Arc::downgrade(actor),
            token,
            authority: authority.clone(),
            base,
            data: authority.configured(),
            associate: authority.permits(Operations::Flat(FlatRights::ASSOCIATE)),
            phase: BlockPhase::Draining,
        }
    }
    pub(super) fn live(&self, window: (PageOffset, PageCount)) -> bool {
        self.authority.alive()
            && self
                .authority
                .limits()
                .operations
                .contains(Operations::Flat(FlatRights::BLOCK))
            && self.authority.contains(window.0, window.1)
            && self.task.upgrade().is_some_and(|task| pend::live(&task))
    }
    pub(super) fn takeover(
        &mut self,
        expected: BlockRef,
        actor: &Arc<Task>,
        token: PendId,
        authority: &Arc<Pend>,
        window: (PageOffset, PageCount),
    ) -> Result<(), PendFail> {
        if self.reference != expected
            || self.live(window)
            || !pend::live(actor)
            || !self.authority.descends_from(authority)
            || !(authority.alive()
                && authority
                    .limits()
                    .operations
                    .contains(Operations::Flat(FlatRights::BLOCK)))
            || !authority.contains(window.0, window.1)
        {
            return Err(PendFail::Denied);
        }
        let generation = expected.generation().next().ok_or(PendFail::OutOfMemory)?;
        if self.authority.configured() == Modifiable::No {
            self.data = Modifiable::No;
        }
        self.associate &= self
            .authority
            .limits()
            .operations
            .contains(Operations::Flat(FlatRights::ASSOCIATE));
        self.reference =
            BlockRef::new(expected.flat(), expected.record(), generation).expect("successor");
        self.actor = actor.ident.id;
        self.task = Arc::downgrade(actor);
        self.token = token;
        self.authority = authority.clone();
        self.phase = BlockPhase::Draining;
        Ok(())
    }
}
