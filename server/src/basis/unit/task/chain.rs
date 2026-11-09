//! Intrusive Task membership. An attached tail is distinct from a detached Task.
//! The container lock protects topology; TaskState owns the single link record.
//! A drained chain retains membership until each element is actually removed.
use super::Task;
use alloc::sync::Arc;

pub(crate) enum Link {
    Detached,
    Attached { next: Option<Arc<Task>> },
}
impl Link {
    pub(super) fn detached(&self) -> bool {
        matches!(self, Self::Detached)
    }
    fn attach(&mut self) {
        assert!(self.detached(), "Task already belongs to a chain");
        *self = Self::Attached { next: None };
    }
    fn detach(&mut self) -> Option<Arc<Task>> {
        match core::mem::replace(self, Self::Detached) {
            Self::Attached { next } => next,
            Self::Detached => panic!("Task is not attached"),
        }
    }
    fn next(&mut self) -> &mut Option<Arc<Task>> {
        match self {
            Self::Attached { next } => next,
            Self::Detached => panic!("Task is not attached"),
        }
    }
}

pub(crate) struct Chain {
    head: Option<Arc<Task>>,
    tail: Option<Arc<Task>>,
}
impl Chain {
    pub(crate) const fn new() -> Self {
        Self {
            head: None,
            tail: None,
        }
    }
    pub(crate) fn enqueue(&mut self, task: Arc<Task>) {
        // Validate membership before mutating either end of this container.
        task.state.lock().link().attach();
        match self.tail.take() {
            None => self.head = Some(task.clone()),
            Some(last) => *last.state.lock().link().next() = Some(task.clone()),
        }
        self.tail = Some(task);
    }
    pub(crate) fn dequeue(&mut self) -> Option<Arc<Task>> {
        let head = self.head.take()?;
        self.head = head.state.lock().link().detach();
        if self.head.is_none() {
            self.tail = None;
        }
        Some(head)
    }
    pub(crate) fn take(&mut self) -> Option<Drain> {
        if self.is_empty() {
            return None;
        }
        Some(Drain(core::mem::replace(self, Self::new())))
    }
    pub(crate) fn remove_if(
        &mut self,
        hit: &mut dyn FnMut(&Arc<Task>) -> bool,
    ) -> Option<Arc<Task>> {
        let mut prev: Option<Arc<Task>> = None;
        let mut cur = self.head.clone();
        while let Some(node) = cur {
            if hit(&node) {
                let next = node.state.lock().link().detach();
                let was_tail = next.is_none();
                match &prev {
                    Some(p) => *p.state.lock().link().next() = next,
                    None => self.head = next,
                }
                if was_tail {
                    self.tail = prev;
                }
                return Some(node);
            }
            cur = node.state.lock().link().next().clone();
            prev = Some(node);
        }
        None
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }
    #[cfg(debug_assertions)]
    pub(crate) fn len(&self) -> usize {
        let mut n = 0;
        let mut cur = self.head.clone();
        while let Some(node) = cur {
            n += 1;
            cur = node.state.lock().link().next().clone();
        }
        n
    }
}
impl Drop for Chain {
    fn drop(&mut self) {
        // Release links iteratively, including when a partial drain is abandoned.
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
