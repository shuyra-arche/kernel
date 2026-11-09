//! Intrusive membership with one owner identity and bidirectional links. A
//! known Task leaves its container in constant work; drains keep their identity.
use super::Task;
use alloc::sync::{Arc, Weak};
use core::sync::atomic::{AtomicUsize, Ordering};

pub(crate) enum Link {
    Detached,
    Attached {
        owner: usize,
        previous: Weak<Task>,
        next: Option<Arc<Task>>,
    },
}
impl Link {
    pub(super) fn detached(&self) -> bool {
        matches!(self, Self::Detached)
    }
}

pub(crate) struct Chain {
    identity: usize,
    head: Option<Arc<Task>>,
    tail: Option<Arc<Task>>,
    len: usize,
}
impl Chain {
    pub(crate) const fn new() -> Self {
        Self {
            identity: 0,
            head: None,
            tail: None,
            len: 0,
        }
    }
    pub(crate) fn enqueue(&mut self, task: Arc<Task>) {
        {
            let mut state = task.state.lock();
            assert!(state.link().detached(), "Task already belongs to a chain");
            if self.identity == 0 {
                static NEXT: AtomicUsize = AtomicUsize::new(1);
                self.identity = NEXT
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                    .expect("chain identity exhausted");
            }
            *state.link() = Link::Attached {
                owner: self.identity,
                previous: self.tail.as_ref().map_or_else(Weak::new, Arc::downgrade),
                next: None,
            };
        }
        match self.tail.take() {
            None => self.head = Some(task.clone()),
            Some(last) => match last.state.lock().link() {
                Link::Attached { next, .. } => *next = Some(task.clone()),
                Link::Detached => panic!("detached chain tail"),
            },
        }
        self.tail = Some(task);
        self.len += 1;
    }
    pub(crate) fn dequeue(&mut self) -> Option<Arc<Task>> {
        let head = self.head.clone()?;
        self.remove(&head)
    }
    pub(crate) fn remove(&mut self, task: &Arc<Task>) -> Option<Arc<Task>> {
        let (previous, next) = {
            let mut state = task.state.lock();
            match state.link() {
                Link::Attached { owner, .. } if *owner == self.identity => {}
                _ => return None,
            }
            let Link::Attached { previous, next, .. } =
                core::mem::replace(state.link(), Link::Detached)
            else {
                unreachable!()
            };
            (previous.upgrade(), next)
        };
        match &previous {
            Some(previous) => match previous.state.lock().link() {
                Link::Attached { next: link, .. } => *link = next.clone(),
                Link::Detached => panic!("detached chain predecessor"),
            },
            None => self.head = next.clone(),
        }
        match next {
            Some(next) => match next.state.lock().link() {
                Link::Attached { previous: link, .. } => {
                    *link = previous.as_ref().map_or_else(Weak::new, Arc::downgrade)
                }
                Link::Detached => panic!("detached chain successor"),
            },
            None => self.tail = previous,
        }
        self.len -= 1;
        Some(task.clone())
    }
    pub(crate) fn take(&mut self) -> Option<Drain> {
        if self.is_empty() {
            None
        } else {
            Some(Drain(core::mem::replace(self, Self::new())))
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}
impl Drop for Chain {
    fn drop(&mut self) {
        while self.dequeue().is_some() {}
    }
}
pub(crate) struct Drain(Chain);
impl Drain {
    pub(crate) fn single(task: Arc<Task>) -> Self {
        let mut chain = Chain::new();
        chain.enqueue(task);
        Self(chain)
    }
}
impl Iterator for Drain {
    type Item = Arc<Task>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.dequeue()
    }
}
