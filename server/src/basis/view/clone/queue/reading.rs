use super::super::payload::Payload;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use kernel_api::abi::call::clone_view::CloneFail;
use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::identity::TaskId;

use super::super::CloneView;
use super::super::HANDS_LIVE;
use super::super::key;
use super::super::note_hand_off;
use super::Pending;
use super::Slot;
use crate::basis::sign;

pub(crate) struct Reading<'a> {
    meta: &'a CloneView,
    pub(crate) from: TaskId,
    pub(crate) bytes: Arc<Payload>,
    finished: bool,
}

impl Reading<'_> {
    /// Caller serializes its Pend table and holds work commit. Validate queue
    /// identity, publish prepared authority and consume under one queue guard.
    /// The callback must be infallible, allocation-free, and never inspect this queue.
    pub(crate) fn commit(&mut self, publish: impl FnOnce()) -> Result<(), CloneFail> {
        let hand = {
            let mut pending = self.meta.pending.lock();
            let Pending::Queue(q) = &mut *pending else {
                return Err(CloneFail::Dead);
            };
            if !q.taking
                || !q
                    .head()
                    .is_some_and(|head| Arc::ptr_eq(&head.buf, &self.bytes))
            {
                return Err(CloneFail::Busy);
            }
            publish();
            let Some(Slot::Ready(hand)) = q.hands.pop_front() else {
                unreachable!("validated queue head")
            };
            q.bytes -= hand.buf.len();
            q.taking = false;
            self.finished = true;
            hand
        };
        HANDS_LIVE.fetch_sub(1, Ordering::Relaxed);
        note_hand_off(self.meta, hand.from, hand.buf.len(), hand.at);
        Ok(())
    }
}
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        // The caller drops its Task/table/commit guards before this reservation.
        if self.finished {
            let _ = sign::wake(key(self.meta, CloneCondition::Empty), &self.meta.life());
            let _ = sign::wake(key(self.meta, CloneCondition::Push), &self.meta.life());
            let _ = sign::wake(key(self.meta, CloneCondition::Pull), &self.meta.life());
        } else {
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

fn back(meta: &CloneView) {
    {
        let mut pending = meta.pending.lock();
        if let Pending::Queue(q) = &mut *pending {
            q.taking = false;
        }
    }
    let _ = sign::wake(key(meta, CloneCondition::Pull), &meta.life());
}
