use super::payload::Payload;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use kernel_api::{
    abi::call::clone_view::CloneFail, model::clone_view::CloneCondition, model::identity::TaskId,
};

use super::pending::Pending;
use super::slot::Slot;
use super::{CloneView, HANDS_LIVE, key, note_hand_off};
use crate::core::work::sign;

pub(crate) struct Reading<'a> {
    meta: &'a CloneView,
    pub(crate) from: TaskId,
    pub(crate) bytes: Arc<Payload>,
    finished: bool,
}

impl Reading<'_> {
    pub(crate) fn finish(mut self) {
        taken(self.meta);
        self.finished = true;
    }
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        if !self.finished {
            back(self.meta);
        }
    }
}

pub(crate) fn read(meta: &CloneView) -> Result<Reading<'_>, CloneFail> {
    let mut pending = meta.pending.lock();
    let queue = match &mut *pending {
        Pending::Queue(queue) => queue,
        Pending::Dead => return Err(CloneFail::Dead),
    };
    if queue.taking {
        return Err(CloneFail::Busy);
    }
    let head = queue.head().ok_or(CloneFail::Busy)?;
    let reading = Reading {
        meta,
        from: head.from,
        bytes: head.buf.clone(),
        finished: false,
    };
    queue.taking = true;
    Ok(reading)
}

pub(crate) fn peek(meta: &CloneView) -> Result<(usize, TaskId, usize), CloneFail> {
    if !meta.alive() {
        return Err(CloneFail::Dead);
    }
    let pending = meta.pending.lock();
    match &*pending {
        Pending::Queue(q) => match q.head() {
            Some(hand) => Ok((
                hand.buf.len(),
                hand.from,
                q.hands.iter().filter_map(Slot::hand).count(),
            )),
            None => Err(CloneFail::Busy),
        },
        Pending::Dead => Err(CloneFail::Dead),
    }
}

pub(super) fn taken(meta: &CloneView) {
    let mut off = None;
    {
        let mut pending = meta.pending.lock();
        if let Pending::Queue(q) = &mut *pending {
            if q.taking {
                q.taking = false;
                off = q.hands.pop_front().and_then(|slot| match slot {
                    Slot::Ready(hand) => {
                        q.bytes -= hand.buf.len();
                        Some(hand)
                    }
                    Slot::Reserved(_, len) => {
                        q.bytes -= len;
                        None
                    }
                });
            }
        }
    }
    if let Some(hand) = off {
        let len = hand.buf.len();
        HANDS_LIVE.fetch_sub(1, Ordering::Relaxed);
        note_hand_off(meta, hand.from, len, hand.at);
    }
    let _ = sign::wake(key(meta, CloneCondition::Empty), &meta.life());
    let _ = sign::wake(key(meta, CloneCondition::Push), &meta.life());
    let _ = sign::wake(key(meta, CloneCondition::Pull), &meta.life());
}

pub(super) fn back(meta: &CloneView) {
    {
        let mut pending = meta.pending.lock();
        if let Pending::Queue(q) = &mut *pending {
            q.taking = false;
        }
    }
    let _ = sign::wake(key(meta, CloneCondition::Pull), &meta.life());
}
