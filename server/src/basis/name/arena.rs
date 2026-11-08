use alloc::vec::Vec;
use core::marker::PhantomData;

use super::{Generation, Serial};

/// Internal registration identity, qualified by both table and slot lifetime.
/// The domain marker prevents mixing kinds; table identity rejects other owners.
pub(crate) struct Key<K> {
    table: usize,
    slot: usize,
    generation: Generation,
    domain: PhantomData<fn() -> K>,
}
impl<K> Copy for Key<K> {}
impl<K> Clone for Key<K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> PartialEq for Key<K> {
    fn eq(&self, other: &Self) -> bool {
        self.table == other.table && self.slot == other.slot && self.generation == other.generation
    }
}
impl<K> Eq for Key<K> {}

enum State<T> {
    Free(Option<usize>),
    Reserved,
    Occupied(T),
    Retired,
}
struct Slot<T> {
    generation: Generation,
    state: State<T>,
}

/// Reusable registration storage. No authority or hardware lifetime is implied
/// by a valid key. The domain must settle those obligations before removal.
pub(crate) struct Arena<T, K> {
    table: Option<usize>,
    slots: Vec<Slot<T>>,
    free: Option<usize>,
    len: usize,
    domain: PhantomData<fn() -> K>,
}
impl<T, K> Arena<T, K> {
    pub(crate) const fn new() -> Self {
        Self {
            table: None,
            slots: Vec::new(),
            free: None,
            len: 0,
            domain: PhantomData,
        }
    }

    /// Reserve storage before effects. Reserved slots are invisible to readers;
    /// every successful reservation must be published or cancelled by its owner.
    pub(crate) fn reserve(&mut self) -> Option<Key<K>> {
        static TABLES: Serial = Serial::new(1, usize::MAX);
        let table = match self.table {
            Some(table) => table,
            None => {
                let table = TABLES.allocate()?;
                self.table = Some(table);
                table
            }
        };
        let slot = if let Some(slot) = self.free {
            let State::Free(next) = self.slots[slot].state else {
                unreachable!()
            };
            self.free = next;
            self.slots[slot].state = State::Reserved;
            slot
        } else {
            self.slots.try_reserve(1).ok()?;
            let slot = self.slots.len();
            self.slots.push(Slot {
                generation: Generation::first(),
                state: State::Reserved,
            });
            slot
        };
        Some(Key {
            table,
            slot,
            generation: self.slots[slot].generation,
            domain: PhantomData,
        })
    }

    /// Publication performs no allocation and retains the value on invalid keys.
    pub(crate) fn publish(&mut self, key: Key<K>, value: T) -> Result<(), T> {
        if !self.matches(key) || !matches!(self.slots[key.slot].state, State::Reserved) {
            return Err(value);
        }
        self.slots[key.slot].state = State::Occupied(value);
        self.len += 1;
        Ok(())
    }
    pub(crate) fn cancel(&mut self, key: Key<K>) -> bool {
        if !self.matches(key) || !matches!(self.slots[key.slot].state, State::Reserved) {
            return false;
        }
        self.recycle(key.slot);
        true
    }
    pub(crate) fn get(&self, key: Key<K>) -> Option<&T> {
        if !self.matches(key) {
            return None;
        }
        match &self.slots[key.slot].state {
            State::Occupied(value) => Some(value),
            _ => None,
        }
    }
    pub(crate) fn remove(&mut self, key: Key<K>) -> Option<T> {
        if !self.matches(key) || !matches!(self.slots[key.slot].state, State::Occupied(_)) {
            return None;
        }
        let State::Occupied(value) =
            core::mem::replace(&mut self.slots[key.slot].state, State::Retired)
        else {
            unreachable!()
        };
        self.len -= 1;
        self.recycle(key.slot);
        Some(value)
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| match &slot.state {
            State::Occupied(value) => Some(value),
            _ => None,
        })
    }
    pub(crate) fn into_values(self) -> impl Iterator<Item = T> {
        self.slots.into_iter().filter_map(|slot| match slot.state {
            State::Occupied(value) => Some(value),
            _ => None,
        })
    }
    fn matches(&self, key: Key<K>) -> bool {
        self.table == Some(key.table)
            && self
                .slots
                .get(key.slot)
                .is_some_and(|slot| slot.generation == key.generation)
    }
    fn recycle(&mut self, slot: usize) {
        let entry = &mut self.slots[slot];
        if let Some(next) = entry.generation.next() {
            entry.generation = next;
            entry.state = State::Free(self.free);
            self.free = Some(slot);
        } else {
            // Exhaustion is terminal: no old key can become valid again.
            entry.state = State::Retired;
        }
    }
}
