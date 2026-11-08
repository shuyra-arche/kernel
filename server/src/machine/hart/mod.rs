mod id;
mod per_hart;

pub use id::{FirmwareId, HartId};
pub use per_hart::PerHart;
pub(crate) use per_hart::lease_load;
pub(crate) use per_hart::lease_store;
pub use per_hart::per_hart_ptr;
pub(crate) use per_hart::prepare_boot;
pub(crate) use per_hart::request_instruction_sync;
pub use per_hart::set_scheduler;

use ::core::sync::atomic::AtomicUsize;
use ::core::sync::atomic::Ordering;

use crate::machine::board;
use crate::machine::layout::MAX_HART_SLOTS;
use kernel_api::address::VirtAddr;

static ONLINE: [AtomicUsize; MAX_HART_SLOTS / usize::BITS as usize] =
    [const { AtomicUsize::new(0) }; MAX_HART_SLOTS / usize::BITS as usize];

pub(crate) fn mark_online() {
    crate::machine::switcher::context::extension::hardware::initialize();
    let (word, bit) = hart_id().bit();
    ONLINE[word].fetch_or(bit, Ordering::Release);
}

pub(crate) fn online(hart: HartId) -> bool {
    let (word, bit) = hart.bit();
    ONLINE[word].load(Ordering::Acquire) & bit != 0
}

pub fn hart_count() -> usize {
    let n = board::info().hart.count;
    assert!(
        n <= MAX_HART_SLOTS,
        "DTB reports {n} harts, at most {MAX_HART_SLOTS} VA slots"
    );
    n
}

pub(crate) fn wait() {
    unsafe { core::arch::asm!("wfi", options(nostack)) };
}

#[inline]
pub fn hart_id() -> HartId {
    let id: usize;
    unsafe {
        ::core::arch::asm!(
            "ld {0}, 0(tp)",
            out(reg) id,
            options(nomem, nostack, preserves_flags),
        );
    }
    HartId::new(id)
}

#[inline]
pub fn scheduler() -> *mut () {
    let p: usize;
    unsafe {
        ::core::arch::asm!(
            "ld {0}, 0x10(tp)",
            out(reg) p,
            options(nomem, nostack, preserves_flags),
        );
    }
    p as *mut ()
}

#[inline]
pub fn hart_frame() -> VirtAddr {
    let f: usize;
    unsafe {
        ::core::arch::asm!(
            "ld {0}, 0x08(tp)",
            out(reg) f,
            options(nomem, nostack, preserves_flags),
        );
    }
    VirtAddr::wrap(f)
}

/// Current hardware identity is available even before board facts are installed.
#[inline]
pub fn firmware_id() -> FirmwareId {
    let id: usize;
    unsafe {
        core::arch::asm!("ld {0}, 0x28(tp)", out(reg) id, options(nostack, readonly));
    }
    FirmwareId::new(id)
}

pub fn firmware(slot: HartId) -> FirmwareId {
    board::info().hart.firmware(slot)
}
