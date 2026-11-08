//! Indexed min-heap: a ticket has one deadline and can be removed without a scan.
use alloc::vec::Vec;
use hashbrown::HashMap;

pub(crate) struct Queue {
    heap: Vec<(u64, u64)>, // absolute tick, ticket
    positions: HashMap<u64, usize>,
}

impl Queue {
    pub(crate) fn new() -> Self {
        Self {
            heap: Vec::new(),
            positions: HashMap::new(),
        }
    }
    pub(crate) fn nearest(&self) -> Option<u64> {
        self.heap.first().map(|e| e.0)
    }
    pub(crate) fn insert(&mut self, ticket: u64, at: u64) -> Result<(), ()> {
        // Reserve before changing an existing registration; failure is atomic.
        self.heap.try_reserve(1).map_err(|_| ())?;
        self.positions.try_reserve(1).map_err(|_| ())?;
        self.remove(ticket);
        let index = self.heap.len();
        self.heap.push((at, ticket));
        self.positions.insert(ticket, index);
        self.up(index);
        Ok(())
    }
    pub(crate) fn remove(&mut self, ticket: u64) -> bool {
        let Some(index) = self.positions.remove(&ticket) else {
            return false;
        };
        self.heap.swap_remove(index);
        if index < self.heap.len() {
            *self
                .positions
                .get_mut(&self.heap[index].1)
                .expect("heap index") = index;
            if index > 0 && self.heap[index] < self.heap[(index - 1) / 2] {
                self.up(index);
            } else {
                self.down(index);
            }
        }
        true
    }
    pub(crate) fn pop_due(&mut self, now: u64) -> Option<(u64, u64)> {
        let entry = *self.heap.first()?;
        if entry.0 > now {
            return None;
        }
        assert!(self.remove(entry.1));
        Some(entry)
    }
    fn swap(&mut self, a: usize, b: usize) {
        self.heap.swap(a, b);
        *self.positions.get_mut(&self.heap[a].1).expect("heap index") = a;
        *self.positions.get_mut(&self.heap[b].1).expect("heap index") = b;
    }
    fn up(&mut self, mut at: usize) {
        while at > 0 {
            let parent = (at - 1) / 2;
            if self.heap[parent] <= self.heap[at] {
                break;
            }
            self.swap(at, parent);
            at = parent;
        }
    }
    fn down(&mut self, mut at: usize) {
        loop {
            let left = at * 2 + 1;
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.heap.len() && self.heap[right] < self.heap[left] {
                right
            } else {
                left
            };
            if self.heap[at] <= self.heap[child] {
                break;
            }
            self.swap(at, child);
            at = child;
        }
    }
}
