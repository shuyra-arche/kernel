use alloc::collections::VecDeque;

use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::clone_view::CloneLimits;

use super::hand::Hand;
use super::slot::Slot;

#[derive(Default)]
pub(super) struct Queue {
    pub(super) hands: VecDeque<Slot>,
    pub(super) taking: bool,
    pub(super) bytes: usize,
}

impl Queue {
    pub(super) fn ready(&self, dir: CloneCondition, limits: &CloneLimits) -> bool {
        match dir {
            CloneCondition::Pull => self.head().is_some() && !self.taking,
            CloneCondition::Push => {
                self.hands.len() < limits.max_messages && self.bytes < limits.max_bytes
            }
            CloneCondition::Empty => self.hands.is_empty() && !self.taking,
        }
    }

    pub(super) fn head(&self) -> Option<&Hand> {
        self.hands.front().and_then(Slot::hand)
    }
}
