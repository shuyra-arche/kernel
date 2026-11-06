// A detached Rust value is reclaimed after native Reaped, including faults
// before DONE. This list owns heap slots, never native exit facts or receipts.
pub(super) struct Detached {
    pub(super) task: super::TaskId,
    pub(super) slot: usize,
    pub(super) release: unsafe fn(usize),
}
