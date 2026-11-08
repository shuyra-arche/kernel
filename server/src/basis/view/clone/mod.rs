pub(crate) mod payload;
mod queue;

use alloc::sync::Arc;
use alloc::sync::Weak;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::machine::clock;
use crate::machine::sync::Level;
use crate::machine::sync::SpinLock;

use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::clone_view::CloneLimits;
use kernel_api::model::identity::TaskId;

use crate::basis::pend::Need;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;

use queue::Pending;
use queue::Queue;
use queue::Slot;

pub(crate) use queue::{peek, read, reserve_len};

pub struct CloneView {
    pub(crate) id: usize,
    life: Arc<Anchor>,
    owner: TaskId,
    pending: SpinLock<Pending>,
    limits: CloneLimits,
    alarmed: AtomicBool,
}

impl CloneView {
    pub(crate) fn try_new(owner: TaskId) -> Result<Arc<Self>, crate::basis::nest::NestError> {
        Self::try_new_with_limits(owner, CloneLimits::default())
    }

    pub(crate) fn try_new_with_limits(
        owner: TaskId,
        limits: CloneLimits,
    ) -> Result<Arc<Self>, crate::basis::nest::NestError> {
        let life = Anchor::try_new()?;
        Arc::try_new(Self {
            id: super::next_id().map_err(|_| crate::basis::nest::NestError::OutOfMemory)?,
            life,
            owner,
            pending: SpinLock::new_level(Level::L3, Pending::Queue(Queue::default())),
            limits,
            alarmed: AtomicBool::new(false),
        })
        .map_err(|_| crate::basis::nest::NestError::OutOfMemory)
    }

    pub(crate) fn life(&self) -> Weak<Anchor> {
        Arc::downgrade(&self.life)
    }

    pub(crate) fn owner(&self) -> TaskId {
        self.owner
    }

    pub(crate) fn alive(&self) -> bool {
        !matches!(*self.pending.lock(), Pending::Dead)
    }

    /// 这一种等待此刻是否已就绪：队列按队头与库存，计数通知按计数。
    pub(crate) fn ready(&self, dir: CloneCondition) -> bool {
        let pending = self.pending.lock();
        match &*pending {
            Pending::Queue(queue) => queue.ready(dir, &self.limits),
            Pending::Dead => false,
        }
    }
}

impl Drop for CloneView {
    fn drop(&mut self) {
        seal(self);
    }
}

pub(crate) fn key(meta: &CloneView, dir: CloneCondition) -> WakeKey {
    WakeKey::Clone {
        clone_view: meta.id,
        dir,
    }
}

/// 读取队列状态需要读权；发布新数据需要 Modifiable。
pub(crate) fn need(dir: CloneCondition) -> Need {
    match dir {
        CloneCondition::Pull | CloneCondition::Empty => Need::Read,
        CloneCondition::Push => Need::Write,
    }
}

static HOLD_N: AtomicUsize = AtomicUsize::new(0);
static HOLD_MAX_MS: AtomicUsize = AtomicUsize::new(0);
static HOLD_WORST: AtomicUsize = AtomicUsize::new(0);
static HOLD_FROM: AtomicUsize = AtomicUsize::new(0);
static HOLD_OWNER: AtomicUsize = AtomicUsize::new(0);
static HANDS_LIVE: AtomicUsize = AtomicUsize::new(0);
static ALARM_N: AtomicUsize = AtomicUsize::new(0);
const HOLD_MS: usize = 1000;
const ALARM_MAX: usize = 8;

fn elapsed_ms(at: u64) -> usize {
    clock::ticks_to_duration(clock::uptime_ticks().wrapping_sub(at)).as_millis() as usize
}

fn note_hold(meta: &CloneView, from: TaskId, ms: usize) {
    if ms < HOLD_MS {
        return;
    }
    HOLD_N.fetch_add(1, Ordering::Relaxed);
    if ms > HOLD_MAX_MS.load(Ordering::Relaxed) {
        HOLD_MAX_MS.store(ms, Ordering::Relaxed);
        HOLD_WORST.store(meta.id, Ordering::Relaxed);
        HOLD_FROM.store(from.get(), Ordering::Relaxed);
        HOLD_OWNER.store(meta.owner.get(), Ordering::Relaxed);
    }
}

fn alarm_stuck(meta: &CloneView, from: TaskId, len: usize, ms: usize) {
    if ms < HOLD_MS || meta.alarmed.swap(true, Ordering::Relaxed) {
        return;
    }
    if ALARM_N.fetch_add(1, Ordering::Relaxed) >= ALARM_MAX {
        return;
    }
    crate::putln!(
        "view: hand stuck clone_view#{} from={} owner={} len={} age={}ms",
        meta.id,
        from.get(),
        meta.owner.get(),
        len,
        ms,
    );
}

fn note_hand_off(meta: &CloneView, from: TaskId, len: usize, at: u64) {
    if at == 0 {
        return;
    }
    let ms = elapsed_ms(at);
    note_hold(meta, from, ms);
    alarm_stuck(meta, from, len, ms);
}

pub(crate) fn hold_line() {
    crate::putln!(
        "clone_view: push_hold_n={} push_hold_max_ms={} worst=clone_view#{} from={} owner={} live={}",
        HOLD_N.load(Ordering::Relaxed),
        HOLD_MAX_MS.load(Ordering::Relaxed),
        HOLD_WORST.load(Ordering::Relaxed),
        HOLD_FROM.load(Ordering::Relaxed),
        HOLD_OWNER.load(Ordering::Relaxed),
        HANDS_LIVE.load(Ordering::Relaxed),
    );
}

pub(crate) fn seal(meta: &CloneView) {
    let pending = core::mem::replace(&mut *meta.pending.lock(), Pending::Dead);
    if let Pending::Queue(q) = &pending {
        if let Some(head) = q.hands.iter().filter_map(Slot::hand).next() {
            note_hand_off(meta, head.from, head.buf.len(), head.at);
        }
        HANDS_LIVE.fetch_sub(
            q.hands.iter().filter_map(Slot::hand).count(),
            Ordering::Relaxed,
        );
    }
    drop(pending);
    sign::wipe(WakeKey::seal(
        kernel_api::model::view::ViewKind::Clone,
        meta.id,
    ));
    sign::wipe(key(meta, CloneCondition::Pull));
    sign::wipe(key(meta, CloneCondition::Empty));
    sign::wipe(key(meta, CloneCondition::Push));
}
