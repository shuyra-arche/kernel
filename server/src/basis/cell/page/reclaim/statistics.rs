//! Telemetry owned by the retention lifecycle. It is never consulted by reclaim.
use super::Kind;
use crate::metal::sync::SpinLock;
#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub cpu: u64,
    pub external: u64,
    pub retiring: u64,
    pub abandoned: u64,
}
static TOTAL: SpinLock<Snapshot> = SpinLock::new(Snapshot {
    cpu: 0,
    external: 0,
    retiring: 0,
    abandoned: 0,
});
fn add(value: &mut u64, bytes: u64) {
    *value = value.checked_add(bytes).unwrap_or(u64::MAX);
}
fn sub(value: &mut u64, bytes: u64) {
    if *value != u64::MAX {
        *value = value
            .checked_sub(bytes)
            .expect("balanced retention telemetry");
    }
}
pub(super) fn register(kind: Kind, bytes: usize) {
    let mut total = TOTAL.lock();
    add(
        match kind {
            Kind::Cpu => &mut total.cpu,
            Kind::External => &mut total.external,
        },
        bytes as u64,
    );
}
pub(super) fn retire(bytes: usize) {
    add(&mut TOTAL.lock().retiring, bytes as u64);
}
pub(super) fn discharge(kind: Kind, bytes: usize, retiring: bool) {
    let mut total = TOTAL.lock();
    sub(
        match kind {
            Kind::Cpu => &mut total.cpu,
            Kind::External => &mut total.external,
        },
        bytes as u64,
    );
    if retiring {
        sub(&mut total.retiring, bytes as u64);
    }
}
pub(super) fn abandon(bytes: usize) {
    add(&mut TOTAL.lock().abandoned, bytes as u64);
}
pub(crate) fn snapshot() -> Snapshot {
    *TOTAL.lock()
}
