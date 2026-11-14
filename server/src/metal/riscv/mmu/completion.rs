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

static INSTRUCTIONS: [Mailbox; MAX_HART_SLOTS] = [const { Mailbox::new() }; MAX_HART_SLOTS];

/// Service only the observed round. A racing newer request remains pending.
pub(crate) fn service() {
    service_mailbox(&MAILBOXES[hart::hart_id().get()], false);
    service_mailbox(&INSTRUCTIONS[hart::hart_id().get()], true);
}

fn service_mailbox(mailbox: &Mailbox, instructions: bool) {
    if let Some(round) = mailbox.pending() {
        // Acquire observed the initiator's PTE/payload edits. SFENCE excludes
        // further old translations; the explicit data fence orders prior CPU
        // loads/stores before the release acknowledgement observed by reuse.
        unsafe {
            core::arch::asm!(
                "fence rw, rw",
                "sfence.vma",
                "fence rw, rw",
                options(nostack)
            );
            if instructions {
                core::arch::asm!("fence.i", options(nostack));
            }
            mailbox.complete(round);
        }
    }
}

/// No COMMIT/Nest lock may cross this wait. Trap entry itself masks interrupts;
/// cooperative service still lets simultaneous initiators complete each other.
/// Caller serialization (Team operation/Pend ledger) is not used by responders.
/// Failure leaves all ownership with the caller; no Drop retry or receipt.
pub(crate) fn synchronize() -> Result<(), Error> {
    synchronize_mailboxes(&MAILBOXES)
}

/// A data acknowledgement never substitutes for instruction visibility.
pub(crate) fn synchronize_instructions() -> Result<(), Error> {
    hart::request_instruction_sync();
    synchronize_mailboxes(&INSTRUCTIONS)
}

fn synchronize_mailboxes(mailboxes: &[Mailbox; MAX_HART_SLOTS]) -> Result<(), Error> {
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
            mailboxes[slot.get()].request(round);
            if slot != me {
                interrupt::ipi::send_completion(slot).map_err(|_| Error::Fault)?;
            }
        }
    }
    // Bound both elapsed time and polling work, including a stalled timebase.
    let start = crate::metal::clock::now().as_ticks();
    let budget = (crate::entry::boot::facts::info().hart.hertz() as u64).max(1);
    for _ in 0..5_000_000 {
        service();
        let complete = (0..hart::hart_count()).map(hart::HartId::new).all(|slot| {
            let (word, bit) = slot.bit();
            participants[word] & bit == 0 || mailboxes[slot.get()].covers(round)
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
