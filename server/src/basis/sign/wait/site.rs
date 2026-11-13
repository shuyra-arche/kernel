use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use hashbrown::HashMap;

use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::chain::{Chain, Drain};
use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;

use super::key::WakeKey;

pub(in super::super) struct Site {
    pend: bool,
    chain: Chain,
    life: Weak<Anchor>,
    pub(super) fwd: Fwd,
}

impl Site {
    fn new(life: &Weak<Anchor>) -> Self {
        Self {
            pend: false,
            chain: Chain::new(),
            life: life.clone(),
            fwd: Fwd::empty(),
        }
    }

    pub(super) fn ensure<'a>(
        sites: &'a mut HashMap<WakeKey, Site>,
        key: WakeKey,
        life: &Weak<Anchor>,
    ) -> Result<&'a mut Site, ()> {
        if !sites.contains_key(&key) {
            // Admission bounds ordinary bulk wake and shard lookup work.
            if sites.len() == 256 {
                return Err(());
            }
            sites.try_reserve(1).map_err(|_| ())?;
            sites.insert(key, Site::new(life));
        }
        sites.get_mut(&key).ok_or(())
    }

    pub(super) fn admit(&self) -> Result<(), ()> {
        if self.chain.len() < 64 {
            Ok(())
        } else {
            Err(())
        }
    }

    pub(super) fn take_beacon(&mut self) -> bool {
        core::mem::take(&mut self.pend)
    }
    /// Retention is only for notifications without a current recipient.
    pub(super) fn notify(&mut self, all: bool) -> Option<Drain> {
        let chain = self.take(all);
        if chain.is_none() {
            self.pend = true;
        }
        chain
    }
    pub(super) fn take(&mut self, all: bool) -> Option<Drain> {
        if all {
            self.chain.take()
        } else {
            self.chain.dequeue().map(Drain::single)
        }
    }
    pub(super) fn dequeue(&mut self) -> Option<Arc<Task>> {
        self.chain.dequeue()
    }
    pub(super) fn remove(&mut self, task: &Arc<Task>) -> Option<Arc<Task>> {
        self.chain.remove(task)
    }
    pub(super) fn has_waiters(&self) -> bool {
        !self.chain.is_empty()
    }
    /// Initial enrollment consumes a retained notification instead of sleeping.
    pub(super) fn register(&mut self, task: Arc<Task>) -> bool {
        if Anchor::dead(&self.life) || self.take_beacon() {
            return false;
        }
        self.enqueue(task);
        true
    }
    /// A continuation already rechecked its condition and must not spin on the
    /// notification that caused that check.
    pub(super) fn requeue(&mut self, task: Arc<Task>) {
        self.pend = false;
        self.enqueue(task);
    }
    fn enqueue(&mut self, task: Arc<Task>) {
        debug_assert!(
            task.waiting(),
            "站点只收 Blocked 任务，且入链前不得挂在链上"
        );
        assert!(
            self.admit().is_ok(),
            "wait admission must precede Task handoff"
        );
        self.chain.enqueue(task);
    }
}

pub(in super::super) const SITE_SHARDS: usize = 16;
const SITE_SHARDS_MASK: usize = SITE_SHARDS - 1;

#[inline]
fn site_shard(key: WakeKey) -> usize {
    let h = key.fold().wrapping_mul(0x9E3779B97F4A7C15);
    ((h >> 32) ^ h) as usize & SITE_SHARDS_MASK
}

type Shard = SpinLock<HashMap<WakeKey, Site>>;

static SHARDS: OnceLock<Box<[Shard]>> = OnceLock::new();
pub(super) fn existing(key: WakeKey) -> Option<&'static Shard> {
    SHARDS.get().map(|shards| &shards[site_shard(key)])
}
pub(in super::super) fn shard_at(shard: usize) -> &'static Shard {
    let arr: &'static [Shard] = SHARDS.get_or_init(|| {
        let mut v: Vec<Shard> = Vec::with_capacity(SITE_SHARDS);
        for _ in 0..SITE_SHARDS {
            v.push(SpinLock::new_level(Level::L3, HashMap::new()));
        }
        v.into_boxed_slice()
    });
    &arr[shard]
}

pub(super) fn sites(key: WakeKey) -> &'static Shard {
    shard_at(site_shard(key))
}

pub(super) fn take_beacon(key: WakeKey) -> bool {
    let mut sites = sites(key).lock();
    let taken = sites.get_mut(&key).is_some_and(Site::take_beacon);
    if taken {
        prune(&mut sites, key);
    }
    taken
}

pub(in super::super) fn prune(sites: &mut HashMap<WakeKey, Site>, key: WakeKey) {
    if let Some(site) = sites.get(&key)
        && site.chain.is_empty()
        && (Anchor::dead(&site.life) || (!site.pend && site.fwd.is_empty()))
    {
        sites.remove(&key);
    }
}

const FWD_MAX: usize = 8;

#[derive(Clone)]
pub(super) struct Fwd {
    ids: [usize; FWD_MAX],
    lives: [Weak<Anchor>; FWD_MAX],
    len: usize,
}

impl Fwd {
    pub(super) fn empty() -> Self {
        Self {
            ids: [0; FWD_MAX],
            lives: core::array::from_fn(|_| Weak::new()),
            len: 0,
        }
    }

    pub(super) fn attach(&mut self, endpoint: usize, life: Weak<Anchor>) -> Result<(), ()> {
        if self.ids[..self.len].contains(&endpoint) {
            return Ok(());
        }
        if self.len == FWD_MAX {
            return Err(());
        }
        self.ids[self.len] = endpoint;
        self.lives[self.len] = life;
        self.len += 1;
        Ok(())
    }

    pub(super) fn detach(&mut self, endpoint: usize) {
        if let Some(at) = self.ids[..self.len].iter().position(|&i| i == endpoint) {
            self.ids.copy_within(at + 1..self.len, at);
            for i in at..self.len - 1 {
                self.lives.swap(i, i + 1);
            }
            self.lives[self.len - 1] = Weak::new();
            self.len -= 1;
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = (usize, &Weak<Anchor>)> {
        self.ids[..self.len]
            .iter()
            .copied()
            .zip(self.lives[..self.len].iter())
    }
}
