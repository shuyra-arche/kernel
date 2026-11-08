use super::super::payload::Payload;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;

use crate::machine::clock;

use kernel_api::abi::call::clone_view::CloneFail;
use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::identity::TaskId;

use super::super::CloneView;
use super::super::HANDS_LIVE;
use super::super::key;
use super::Hand;
use super::Pending;
use super::Slot;
use crate::basis::sign;

pub(crate) struct Reservation<'a> {
    meta: &'a CloneView,
    id: Option<usize>,
}

impl Reservation<'_> {
    pub(crate) fn commit(mut self, buf: Arc<Payload>, from: TaskId) -> Result<(), CloneFail> {
        if buf.is_empty() {
            return Err(CloneFail::Denied);
        }
        let at = clock::uptime_ticks();
        {
            let mut pending = self.meta.pending.lock();
            let Pending::Queue(q) = &mut *pending else {
                return Err(CloneFail::Dead);
            };
            let at_slot = q
                .hands
                .iter()
                .position(|slot| matches!(slot, Slot::Reserved(id, _) if Some(*id) == self.id))
                .ok_or(CloneFail::Dead)?;
            let reserved_len = match q.hands[at_slot] {
                Slot::Reserved(_, len) => len,
                _ => unreachable!(),
            };
            if buf.len() > self.meta.limits.max_len {
                return Err(CloneFail::Denied);
            }
            if reserved_len != 0 && reserved_len != buf.len() {
                return Err(CloneFail::Denied);
            }
            let next_bytes = q.bytes - reserved_len;
            if buf.len() > self.meta.limits.max_bytes.saturating_sub(next_bytes) {
                return Err(CloneFail::Busy);
            }
            q.bytes = next_bytes + buf.len();
            q.hands[at_slot] = Slot::Ready(Hand { from, buf, at });
            HANDS_LIVE.fetch_add(1, Ordering::Relaxed);
            self.id = None;
        }
        let _ = sign::wake(key(self.meta, CloneCondition::Pull), &self.meta.life());
        Ok(())
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else {
            return;
        };
        let removed = {
            let mut pending = self.meta.pending.lock();
            match &mut *pending {
                Pending::Queue(q) => {
                    let at = q
                        .hands
                        .iter()
                        .position(|slot| matches!(slot, Slot::Reserved(known, _) if *known == id));
                    if let Some(Slot::Reserved(_, len)) = at.and_then(|at| q.hands.remove(at)) {
                        q.bytes -= len;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            }
        };
        if removed {
            let _ = sign::wake(key(self.meta, CloneCondition::Pull), &self.meta.life());
            let _ = sign::wake(key(self.meta, CloneCondition::Empty), &self.meta.life());
            let _ = sign::wake(key(self.meta, CloneCondition::Push), &self.meta.life());
        }
    }
}

pub(crate) fn reserve_len(meta: &CloneView, len: usize) -> Result<Reservation<'_>, CloneFail> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    let mut pending = meta.pending.lock();
    let q = match &mut *pending {
        Pending::Queue(q) => q,
        Pending::Dead => return Err(CloneFail::Dead),
    };
    if len > meta.limits.max_len {
        return Err(CloneFail::Denied);
    }
    if q.hands.len() >= meta.limits.max_messages
        || len > meta.limits.max_bytes.saturating_sub(q.bytes)
    {
        return Err(CloneFail::Busy);
    }
    q.hands.try_reserve(1).map_err(|_| CloneFail::OutOfMemory)?;
    let id = NEXT.allocate().ok_or(CloneFail::OutOfMemory)?;
    q.hands.push_back(Slot::Reserved(id, len));
    q.bytes += len;
    Ok(Reservation { meta, id: Some(id) })
}
