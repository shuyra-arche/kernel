#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};
static REPLY: AtomicUsize = AtomicUsize::new(0);
static REPLY1: AtomicUsize = AtomicUsize::new(0);
static REPLY2: AtomicUsize = AtomicUsize::new(0);
static SLOT: AtomicUsize = AtomicUsize::new(0);

unsafe fn transport(slot: usize, _: [usize; 6]) -> (usize, usize, usize) {
    SLOT.store(slot, Ordering::Relaxed);
    (
        REPLY.load(Ordering::Relaxed),
        REPLY1.load(Ordering::Relaxed),
        REPLY2.load(Ordering::Relaxed),
    )
}

// No `env`, `wire`, ABI types, or failure constructors exist at the fixture root.
// `$crate` must resolve every owner reference through this renamed dependency.
renamed_api::__kernel_client_tole!(crate::transport);
renamed_api::__kernel_client_memory!(crate::transport);
renamed_api::__kernel_client_unit!(crate::transport);
renamed_api::__kernel_client_chrono!(crate::transport);

#[test]
fn renamed_owner_generates_typed_effects_without_a_second_type_owner() {
    let _: fn(
        renamed_api::PieToken,
        renamed_api::Wait,
    ) -> renamed_api::ToleResult<renamed_api::ToleAwaitReply> = tole::await_;
    let _: fn(
        kernel_client::PieToken,
        kernel_client::Wait,
    ) -> kernel_client::ToleResult<kernel_client::ToleAwaitReply> =
        kernel_client::call::tole::await_;
    let identity = |id: kernel_client::TaskId| -> renamed_api::TaskId { id };
    assert_eq!(identity(kernel_client::TaskId::new(7)).get(), 7);
    REPLY.store(0, Ordering::Relaxed);
    assert_eq!(
        tole::await_(renamed_api::PieToken::mint(2), renamed_api::Wait::POLL).unwrap(),
        renamed_api::ToleAwaitReply::Pending
    );
    assert_eq!(SLOT.load(Ordering::Relaxed), (12 << 32) | 4);
    REPLY.store(
        renamed_api::MemoryFail::Busy.code() as usize,
        Ordering::Relaxed,
    );
    assert_eq!(
        memory::release(renamed_api::FlatReference::from_bytes(&1usize.to_le_bytes()).unwrap())
            .unwrap_err()
            .source,
        renamed_api::MemoryFail::Busy
    );
    // A real ret3 value preserves both the task identity and third-word reason.
    REPLY.store(renamed_api::ExitCause::Fault as usize, Ordering::Relaxed);
    REPLY1.store(7, Ordering::Relaxed);
    REPLY2.store(99, Ordering::Relaxed);
    assert_eq!(
        unit::join(
            renamed_api::TaskId::new(7).into(),
            renamed_api::Wait::POLL,
            false
        )
        .unwrap(),
        renamed_api::JoinReply::Reaped(renamed_api::TaskExit {
            task: renamed_api::TaskId::new(7),
            cause: renamed_api::ExitCause::Fault,
            reason: 99,
        })
    );
    REPLY.store(23, Ordering::Relaxed);
    assert_eq!(unit::self_id().get(), 23); // Infallible variant in a fallible domain.
    // Infallible clocks use the whole return word, including its high bit.
    REPLY.store(usize::MAX, Ordering::Relaxed);
    assert_eq!(chrono::ticks(), usize::MAX);
    assert_eq!(chrono::clock(), u64::MAX);
}
