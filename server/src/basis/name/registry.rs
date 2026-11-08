use core::hash::Hash;
use hashbrown::HashMap;

use super::{Arena, Key};

/// Stable public identities index internal reusable registrations. Only the
/// arena stores values; the index is never a second authority record. Callers
/// own public-ID allocation and serialize all operations on this registry.
pub(crate) struct Registry<I, T> {
    index: HashMap<I, Key<I>>,
    arena: Arena<T, I>,
}
impl<I: Copy + Eq + Hash, T> Default for Registry<I, T> {
    fn default() -> Self {
        Self {
            index: HashMap::new(),
            arena: Arena::new(),
        }
    }
}
impl<I: Copy + Eq + Hash, T> Registry<I, T> {
    pub(crate) fn reserve(&mut self, id: I) -> Option<Key<I>> {
        if self.index.contains_key(&id) {
            return None;
        }
        self.index.try_reserve(1).ok()?;
        let key = self.arena.reserve()?;
        self.index.insert(id, key);
        Some(key)
    }
    pub(crate) fn publish(&mut self, id: I, key: Key<I>, value: T) -> Result<(), T> {
        if self.index.get(&id) != Some(&key) {
            return Err(value);
        }
        self.arena.publish(key, value)
    }
    pub(crate) fn cancel(&mut self, id: I, key: Key<I>) -> bool {
        if self.index.get(&id) != Some(&key) || !self.arena.cancel(key) {
            return false;
        }
        self.index.remove(&id);
        true
    }
    pub(crate) fn insert(&mut self, id: I, value: T) -> Result<(), T> {
        let Some(key) = self.reserve(id) else {
            return Err(value);
        };
        // No mutation can intervene while &mut self is held.
        assert!(self.publish(id, key, value).is_ok());
        Ok(())
    }
    pub(crate) fn get(&self, id: I) -> Option<&T> {
        self.arena.get(*self.index.get(&id)?)
    }
    pub(crate) fn remove(&mut self, id: I) -> Option<T> {
        let value = self.arena.remove(*self.index.get(&id)?)?;
        self.index.remove(&id);
        Some(value)
    }
    pub(crate) fn len(&self) -> usize {
        self.arena.len()
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.arena.iter()
    }
    pub(crate) fn into_values(self) -> impl Iterator<Item = T> {
        self.arena.into_values()
    }
}
