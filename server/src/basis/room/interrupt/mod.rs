use crate::basis::view::CloneView;
use crate::metal::sync::OnceLock;
use ::core::sync::atomic::AtomicUsize;
use ::core::sync::atomic::Ordering;
use alloc::sync::Arc;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::TaskId;

pub(crate) struct Interrupts {
    pub page_fault: Arc<CloneView>,
}

static INTERRUPTS: OnceLock<Interrupts> = OnceLock::new();
static CLAIMED: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn init() -> Result<(), PendFail> {
    if INTERRUPTS.get().is_some() {
        return Ok(());
    }
    let interrupts = Interrupts {
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

fn note_external() {
    CLAIMED.fetch_add(1, Ordering::Relaxed);
}
pub(crate) fn claimed() -> usize {
    CLAIMED.load(Ordering::Relaxed)
}

mod router;
pub(crate) mod source;
pub(crate) use router::{Router, install, register};
static EXTERNAL: OnceLock<Router> = OnceLock::new();
pub(crate) fn external() -> Option<&'static Router> {
    EXTERNAL.get()
}

/// Registration is preflighted while the recipient ledger and COMMIT are held.
pub(crate) fn dispatch(
    task: &Arc<crate::basis::unit::task::Task>,
    entry: &Arc<crate::basis::pend::PendEntry>,
) -> Result<(), PendFail> {
    if let Some(sign) = entry.target().sign()
        && let Some(source) = external().and_then(|r| r.source(&sign))
    {
        source.bind(task, entry)?;
    }
    Ok(())
}
pub(crate) fn authorize(
    sign: &crate::basis::sign::endpoint::Endpoint,
    task: &crate::basis::unit::task::Task,
    token: kernel_api::model::identity::PendId,
) -> Result<(), kernel_api::abi::call::sign::SignFail> {
    if let Some(source) = external().and_then(|r| r.source(sign)) {
        source.authorize(task, token)?;
    }
    Ok(())
}
pub(crate) fn withdraw(token: kernel_api::model::identity::PendId) {
    if let Some(route) = external() {
        let _commit = crate::basis::commit();
        route.withdraw(token);
    }
}
pub(crate) fn region(sign: &crate::basis::sign::endpoint::Endpoint) -> Option<usize> {
    external()?.source(sign).map(|s| s.region)
}
pub(crate) fn identifies(
    sign: &crate::basis::sign::endpoint::Endpoint,
    controller: u64,
    line: u32,
) -> bool {
    external()
        .and_then(|r| r.source(sign))
        .is_some_and(|s| s.irq.controller as u64 == controller && s.irq.line == line)
}

pub(crate) fn consumed(endpoint: &crate::basis::sign::endpoint::Endpoint) {
    if let Some(router) = external()
        && router.source(endpoint).is_some()
    {
        router.request();
    }
}
