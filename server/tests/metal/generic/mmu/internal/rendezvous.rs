//! Software receipts only; real fences and access retirement need the RV64 suite.
use super::cpu::completion::{Mailbox, Sequence};

#[test]
fn delayed_duplicate_and_old_acknowledgements_cannot_complete_a_new_round() {
    let sequence = Sequence::new();
    let hart = Mailbox::new();
    let old = sequence.next().unwrap();
    hart.request(old);
    let observed = hart.pending().unwrap();
    let new = sequence.next().unwrap();
    hart.request(new);
    unsafe { hart.complete(observed) };
    assert!(hart.covers(old));
    assert!(!hart.covers(new));
    assert_eq!(hart.pending(), Some(new));
    unsafe { hart.complete(observed) };
    assert!(!hart.covers(new));
    unsafe { hart.complete(hart.pending().unwrap()) };
    // A late completion must not regress the receipt either.
    unsafe { hart.complete(observed) };
    assert!(hart.covers(new));
    assert_eq!(hart.pending(), None);
}

#[test]
fn a_newer_global_round_covers_older_edits_but_never_the_reverse() {
    let sequence = Sequence::new();
    let hart = Mailbox::new();
    let first = sequence.next().unwrap();
    let second = sequence.next().unwrap();
    hart.request(second);
    hart.request(first); // Out-of-order initiator publication cannot lose work.
    assert_eq!(hart.pending(), Some(second));
    unsafe { hart.complete(hart.pending().unwrap()) };
    assert!(hart.covers(first) && hart.covers(second));
    let third = sequence.next().unwrap();
    assert!(!hart.covers(third));
}

#[test]
fn simultaneous_initiators_preserve_every_request_without_an_owner_lock() {
    use std::sync::{Arc, Barrier};
    let sequence = Arc::new(Sequence::new());
    let harts = Arc::new([
        Mailbox::new(),
        Mailbox::new(),
        Mailbox::new(),
        Mailbox::new(),
    ]);
    let start = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (sequence, harts, start) = (sequence.clone(), harts.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                let round = sequence.next().unwrap();
                for hart in harts.iter() {
                    hart.request(round);
                }
                round
            })
        })
        .collect();
    let rounds: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    for hart in harts.iter() {
        unsafe { hart.complete(hart.pending().unwrap()) };
    }
    for round in rounds {
        assert!(harts.iter().all(|hart| hart.covers(round)));
    }
}

#[test]
fn data_completion_cannot_cover_a_pending_instruction_round() {
    let sequence = Sequence::new();
    let data = Mailbox::new();
    let instructions = Mailbox::new();
    let code = sequence.next().unwrap();
    instructions.request(code);
    let later_data = sequence.next().unwrap();
    data.request(later_data);
    // Model the receipts; this is not execution evidence for either fence.
    unsafe { data.complete(later_data) };
    assert!(data.covers(code));
    assert!(!instructions.covers(code));
    unsafe { instructions.complete(code) };
    assert!(instructions.covers(code));
    assert!(!instructions.covers(later_data));
}
