use crate::basis::sign::endpoint::Endpoint;
use crate::basis::view::CloneView;
use crate::machine::sync::OnceLock;
use ::core::sync::atomic::AtomicUsize;
use ::core::sync::atomic::Ordering;
use alloc::sync::Arc;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::TaskId;

pub(crate) struct Interrupts {
    pub supervisor_external: Arc<Endpoint>,
    pub page_fault: Arc<CloneView>,
}

static INTERRUPTS: OnceLock<Interrupts> = OnceLock::new();
static RING: AtomicUsize = AtomicUsize::new(0);
static BUSY: AtomicUsize = AtomicUsize::new(0);
static IDLE_RING: AtomicUsize = AtomicUsize::new(0);
static IDLE_BUSY: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn init() -> Result<(), PendFail> {
    if INTERRUPTS.get().is_some() {
        return Ok(());
    }
    let interrupts = Interrupts {
        supervisor_external: Endpoint::try_new(TaskId::new(0))
            .map_err(|_| PendFail::OutOfMemory)?,
        page_fault: CloneView::try_new(TaskId::new(0)).map_err(|_| PendFail::OutOfMemory)?,
    };
    assert!(
        INTERRUPTS.set(interrupts).is_ok(),
        "trap resources already initialized"
    );
    Ok(())
}

pub(crate) fn get() -> &'static Interrupts {
    INTERRUPTS.get().expect("trap resources not initialized")
}

pub(crate) fn deliver_external(idle: bool) -> bool {
    let busy = crate::basis::sign::endpoint::trigger(&get().supervisor_external).is_err();
    note_external(busy, idle);
    busy
}

fn note_external(busy: bool, idle: bool) {
    RING.fetch_add(1, Ordering::Relaxed);
    if busy {
        BUSY.fetch_add(1, Ordering::Relaxed);
    }
    if idle {
        IDLE_RING.fetch_add(1, Ordering::Relaxed);
        if busy {
            IDLE_BUSY.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn stats() -> (usize, usize, usize, usize) {
    (
        RING.load(Ordering::Relaxed),
        BUSY.load(Ordering::Relaxed),
        IDLE_RING.load(Ordering::Relaxed),
        IDLE_BUSY.load(Ordering::Relaxed),
    )
}
