//! All-ever-online Hart rendezvous. Responders use no locks, allocation or
//! scheduling state, including while servicing a peer from an IRQ-off wait.
use crate::metal::{
    generic::{
        memory::Error,
        mmu::completion::{Mailbox, Sequence},
    },
    hart, interrupt,
    layout::MAX_HART_SLOTS,
};

static SEQUENCE: Sequence = Sequence::new();
static MAILBOXES: [Mailbox; MAX_HART_SLOTS] = [const { Mailbox::new() }; MAX_HART_SLOTS];

/// Service only the observed round. A racing newer request remains pending.
pub(crate) fn service() {
    let mailbox = &MAILBOXES[hart::hart_id().get()];
    if let Some(round) = mailbox.pending() {
        // Acquire observed the initiator's PTE/payload edits. SFENCE excludes
        // further old translations; the explicit data fence orders prior CPU
        // loads/stores before the release acknowledgement observed by reuse.
        unsafe {
            core::arch::asm!(
                "fence rw, rw",
                "sfence.vma",
                "fence rw, rw",
                "fence.i",
                options(nostack)
            );
            mailbox.complete(round);
        }
    }
}

/// No COMMIT/Nest lock may cross this wait. Trap entry itself masks interrupts;
/// cooperative service still lets simultaneous initiators complete each other.
/// Caller serialization (Team operation/Pend ledger) is not used by responders.
/// Failure leaves all ownership with the caller; no Drop retry or receipt.
pub(crate) fn synchronize() -> Result<(), Error> {
    crate::metal::allocator::assert_allocation_allowed();
    let round = SEQUENCE.next().ok_or(Error::Capacity)?;
    let me = hart::hart_id();
    let mut participants = [0usize; MAX_HART_SLOTS / usize::BITS as usize];
    // Pair with mark_online's fence before its first address-space activation.
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
    for slot in (0..hart::hart_count()).map(hart::HartId::new) {
        if slot == me || hart::online(slot) {
            let (word, bit) = slot.bit();
            participants[word] |= bit;
            MAILBOXES[slot.get()].request(round);
            if slot != me {
                interrupt::ipi::send(slot).map_err(|_| Error::Fault)?;
            }
        }
    }
    // Bound both elapsed time and polling work, including a stalled timebase.
    let start = crate::metal::clock::now().as_ticks();
    let budget = (crate::entry::boot::facts::info().hart.hertz() as u64 / 10).max(1);
    for _ in 0..1_000_000 {
        service();
        let complete = (0..hart::hart_count()).map(hart::HartId::new).all(|slot| {
            let (word, bit) = slot.bit();
            participants[word] & bit == 0 || MAILBOXES[slot.get()].covers(round)
        });
        if complete {
            return Ok(());
        }
        if crate::metal::clock::now().as_ticks().wrapping_sub(start) >= budget {
            break;
        }
        core::hint::spin_loop();
    }
    Err(Error::TimedOut)
}
