//! Queue slots and the two guards that reserve and consume them.

mod reading;
mod reservation;

pub(crate) use reading::{peek, read};
pub(crate) use reservation::reserve_len;

use super::payload::Payload;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use kernel_api::model::identity::TaskId;

use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::clone_view::CloneLimits;

pub(super) struct Hand {
    pub(super) from: TaskId,
    pub(super) buf: Arc<Payload>,
    pub(super) at: u64,
}

pub(super) enum Slot {
    Reserved(usize, usize),
    Ready(Hand),
    Reading(Hand),
}

impl Slot {
    pub(super) fn hand(&self) -> Option<&Hand> {
        match self {
            Self::Ready(hand) | Self::Reading(hand) => Some(hand),
            Self::Reserved(..) => None,
        }
    }
}

pub(super) enum Pending {
    Queue(Queue),
    Dead,
}

#[derive(Default)]
pub(super) struct Queue {
    pub(super) hands: VecDeque<Slot>,
    pub(super) bytes: usize,
}

impl Queue {
    pub(super) fn ready(&self, dir: CloneCondition, limits: &CloneLimits) -> bool {
        match dir {
            CloneCondition::Pull => matches!(self.hands.front(), Some(Slot::Ready(_))),
            CloneCondition::Push => {
                self.hands.len() < limits.max_messages && self.bytes < limits.max_bytes
            }
            CloneCondition::Empty => self.hands.is_empty(),
        }
    }

    pub(super) fn head(&self) -> Option<&Hand> {
        self.hands.front().and_then(Slot::hand)
    }
}
