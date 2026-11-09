//! Retained allocation-free notification work for admitted producers. Ordinary
//! wait continuations are serviced on a housekeeping Hart, outside RT execution.
use super::*;
pub(super) enum Delivery {
    Idle,
    Queued { next: Option<Arc<Endpoint>> },
    Running,
}
struct Queue {
    first: Option<Arc<Endpoint>>,
    last: Option<Arc<Endpoint>>,
}
static PENDING: SpinLock<Queue> = SpinLock::new(Queue {
    first: None,
    last: None,
});
impl Queue {
    fn enqueue(&mut self, endpoint: Arc<Endpoint>) {
        *endpoint.delivery.lock() = Delivery::Queued { next: None };
        if let Some(last) = self.last.take() {
            let mut state = last.delivery.lock();
            let Delivery::Queued { next } = &mut *state else {
                unreachable!()
            };
            *next = Some(endpoint.clone());
        } else {
            self.first = Some(endpoint.clone());
        }
        self.last = Some(endpoint);
    }
    fn dequeue(&mut self) -> Option<Arc<Endpoint>> {
        let endpoint = self.first.take()?;
        let Delivery::Queued { next } =
            core::mem::replace(&mut *endpoint.delivery.lock(), Delivery::Running)
        else {
            unreachable!()
        };
        self.first = next;
        if self.first.is_none() {
            self.last = None;
        }
        Some(endpoint)
    }
}
pub(crate) fn enqueue(endpoint: &Arc<Endpoint>) {
    let _commit = crate::basis::commit();
    let mut queue = PENDING.lock();
    if matches!(*endpoint.delivery.lock(), Delivery::Idle) {
        queue.enqueue(endpoint.clone());
    }
    drop(queue);
    crate::basis::room::scheduler::contract::notify();
    crate::basis::room::scheduler::idle::nudge(crate::metal::hart::HartId::new(0));
}
pub(crate) fn advance() {
    if crate::basis::room::scheduler::isolated(crate::metal::hart::hart_id())
        || PENDING.lock().first.is_none()
    {
        return;
    }
    let _commit = crate::basis::commit();
    for _ in 0..4 {
        let Some(endpoint) = PENDING.lock().dequeue() else {
            break;
        };
        if super::announce(&endpoint) {
            PENDING.lock().enqueue(endpoint);
        } else {
            *endpoint.delivery.lock() = Delivery::Idle;
        }
    }
    if PENDING.lock().first.is_some() {
        crate::basis::room::deadline::retry();
    }
}
