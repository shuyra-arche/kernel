use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;

use crate::basis::unit::anchor::Anchor;
use crate::metal::sync::SpinLock;

pub(crate) mod exit;
mod state;

use super::Member;
pub(super) use exit::ExitPolicy;
pub(super) use state::TeamStage;
pub(crate) use state::{TeamPhase, TeamState};

pub(crate) struct TeamNode {
    pub id: TeamId,
    pub owner: TaskId,
    pub parent: Option<Arc<TeamNode>>,
    pub state: SpinLock<TeamState>,
    pub tasks: SpinLock<Vec<Arc<Member>>>,
    pub children: SpinLock<Vec<Arc<TeamNode>>>,
    pub life: Arc<Anchor>,
    pub(crate) exit: ExitPolicy,
}

impl TeamNode {
    pub(crate) fn protected(&self) -> bool {
        self.exit.protected() || self.parent.as_ref().is_some_and(|p| p.protected())
    }
    pub fn paused(&self) -> bool {
        let local = matches!(self.state.lock().phase, TeamPhase::Paused);
        local || self.parent.as_ref().is_some_and(|p| p.paused())
    }
    pub(crate) fn admitting(&self) -> bool {
        let state = self.state.lock();
        let local = state.admission.permits() && !state.terminal();
        drop(state);
        local && self.parent.as_ref().is_none_or(|parent| parent.admitting())
    }
    pub fn closed(&self) -> bool {
        let local = self.state.lock().terminal();
        local || self.parent.as_ref().is_some_and(|p| p.closed())
    }
    pub fn member(&self, id: TaskId) -> Option<Arc<Member>> {
        self.tasks.lock().iter().find(|m| m.id == id).cloned()
    }
    pub fn visit(&self, run: &mut impl FnMut(&TeamNode)) {
        run(self);
        let mut after = 0;
        loop {
            let child = {
                let children = self.children.lock();
                children
                    .get(children.partition_point(|c| c.id.get() <= after))
                    .cloned()
            };
            let Some(child) = child else { break };
            after = child.id.get();
            child.visit(run);
        }
    }
    pub fn all_reaped(&self) -> bool {
        let mut done = true;
        self.visit(&mut |node| {
            if node.tasks.lock().iter().any(|m| m.exit().is_none()) {
                done = false;
            }
        });
        done
    }
    pub fn find(&self, id: TaskId) -> Option<Arc<Member>> {
        let mut found = None;
        self.visit(&mut |node| {
            if found.is_none() {
                found = node.member(id);
            }
        });
        found
    }
    pub fn clear(&self, owner: TaskId, root: Option<TeamId>) {
        let _commit = crate::basis::commit();
        self.visit(&mut |node| {
            node.tasks.lock().retain(|member| {
                member.clear(owner, root);
                member.retained()
            });
            node.prune();
        });
    }

    /// Membership is sorted and bounded by MAX_MEMBERS. Only the changed
    /// member can become removable; unrelated Teams need no cleanup scan.
    pub(super) fn remove(&self, id: TaskId) {
        let _commit = crate::basis::commit();
        {
            let mut members = self.tasks.lock();
            let Ok(at) = members.binary_search_by_key(&id.get(), |member| member.id.get()) else {
                return;
            };
            if members[at].retained() {
                return;
            }
            members.remove(at);
        }
        self.prune();
    }

    /// Empty, ousted nodes detach upward only. The ancestry has MAX_DEPTH
    /// entries; sibling subtrees are never visited or recursively pruned.
    pub fn prune(&self) {
        let _commit = crate::basis::commit();
        let mut node = self;
        loop {
            if !node.state.lock().phase.ousted()
                || !node.tasks.lock().is_empty()
                || !node.children.lock().is_empty()
            {
                return;
            }
            let Some(parent) = node.parent.as_deref() else {
                return;
            };
            let removed = {
                let mut children = parent.children.lock();
                let Ok(at) = children.binary_search_by_key(&node.id.get(), |child| child.id.get())
                else {
                    // Construction failed before publication, or already detached.
                    return;
                };
                children.remove(at)
            };
            drop(removed);
            node = parent;
        }
    }
    pub fn doom(&self) {
        let _commit = crate::basis::commit();
        self.visit(&mut |node| {
            let mut state = node.state.lock();
            if !state.terminal() {
                state.phase = TeamPhase::Doomed;
            }
        });
    }
    pub fn notify(&self) {
        let mut node = Some(self);
        while let Some(here) = node {
            crate::basis::sign::signal(crate::basis::sign::WakeKey::Team { id: here.id });
            node = here.parent.as_deref();
        }
    }
}
