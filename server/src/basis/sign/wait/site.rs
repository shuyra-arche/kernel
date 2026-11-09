use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use hashbrown::HashMap;

use crate::basis::room::chain::Chain;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::task::Task;
use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;

use super::key::WakeKey;

pub(in super::super) struct Site {
    pub(super) pend: bool,
    pub(in super::super) chain: Chain,
    pub(super) life: Weak<Anchor>,
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
            sites.try_reserve(1).map_err(|_| ())?;
            sites.insert(key, Site::new(life));
        }
        sites.get_mut(&key).ok_or(())
    }

    pub(super) fn push_back(&mut self, task: Arc<Task>) {
        debug_assert!(
            task.waiting() && task.next().is_none(),
            "站点只收 Blocked 任务，且入链前不得挂在链上"
        );
        self.chain.push(task);
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
    let taken = match sites.get_mut(&key) {
        Some(site) if site.pend => {
            site.pend = false;
            true
        }
        _ => false,
    };
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
