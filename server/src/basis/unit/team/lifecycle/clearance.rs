//! Receipt withdrawal keeps a monotonic position, not a subtree snapshot.
use super::TeamNode;
use alloc::sync::Arc;
use kernel_api::model::identity::{TaskId, TeamId};

enum Position {
    Members(usize),
    Children(usize),
    Finished,
}
pub(crate) struct Clearance {
    root: TeamId,
    node: Arc<TeamNode>,
    position: Position,
}
impl Clearance {
    pub(crate) fn new(node: Arc<TeamNode>) -> Self {
        Self {
            root: node.id,
            node,
            position: Position::Members(0),
        }
    }
    /// One member, child, or ancestor edge per attempt. Removal of an earlier
    /// member/child cannot move this cursor past an unvisited identity.
    pub(crate) fn advance(&mut self, owner: TaskId, root: Option<TeamId>) -> bool {
        let _commit = crate::basis::commit();
        match self.position {
            Position::Members(after) => {
                let member = {
                    let members = self.node.tasks.lock();
                    members
                        .get(members.partition_point(|m| m.id.get() <= after))
                        .cloned()
                };
                if let Some(member) = member {
                    self.position = Position::Members(member.id.get());
                    member.clear(owner, root);
                    self.node.remove(member.id);
                } else {
                    self.position = Position::Children(0);
                }
            }
            Position::Children(after) => {
                let child = {
                    let children = self.node.children.lock();
                    children
                        .get(children.partition_point(|c| c.id.get() <= after))
                        .cloned()
                };
                if let Some(child) = child {
                    self.node = child;
                    self.position = Position::Members(0);
                } else if self.node.id == self.root {
                    self.node.prune();
                    self.position = Position::Finished;
                    return true;
                } else {
                    let parent = self
                        .node
                        .parent
                        .as_ref()
                        .expect("clearance root ancestor")
                        .clone();
                    self.position = Position::Children(self.node.id.get());
                    self.node.prune();
                    self.node = parent;
                }
            }
            Position::Finished => return true,
        }
        false
    }
}
