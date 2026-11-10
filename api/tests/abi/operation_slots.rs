#[test]
fn every_operation_keeps_its_published_slot() {
    assert_eq!(
        <kernel_api::abi::call::debug::operations::Put as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 34359738368u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::debug::operations::Get as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 34359738369u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Create as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 21474836480u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Delete as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 21474836481u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Push as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 21474836482u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Pull as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 21474836488u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Peek as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 21474836484u64 as usize)
    );
    assert_eq!(<kernel_api::abi::call::cell::operations::Create as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 0);
    assert_eq!(<kernel_api::abi::call::cell::operations::Delete as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 1);
    assert_eq!(<kernel_api::abi::call::cell::operations::Push as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 2);
    assert_eq!(<kernel_api::abi::call::cell::operations::Pull as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 3);
    assert_eq!(<kernel_api::abi::call::cell::operations::Publish as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 4);
    assert_eq!(<kernel_api::abi::call::cell::operations::Retract as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 5);
    assert_eq!(<kernel_api::abi::call::cell::operations::Protect as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 6);
    assert_eq!(<kernel_api::abi::call::cell::operations::Inspect as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (2usize << 32) | 7);
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Accord as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771086u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Narrow as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771088u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Revoke as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771078u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Return as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771087u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Inspect as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771089u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Collect as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 30064771090u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Create as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 42949672960u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Delete as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 42949672961u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Open as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 42949672962u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Shut as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 42949672963u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Yield as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 0u64 as usize)
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::SleepUntil as kernel_api::abi::operation::RegisterCall>::SLOT,
        (kernel_api::abi::PREFIX | 1u64 as usize)
    );
    assert_eq!(<kernel_api::abi::call::sign::operations::Create as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 0);
    assert_eq!(<kernel_api::abi::call::sign::operations::Delete as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 1);
    assert_eq!(<kernel_api::abi::call::sign::operations::Listen as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 2);
    assert_eq!(<kernel_api::abi::call::sign::operations::Trigger as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 3);
    assert_eq!(<kernel_api::abi::call::sign::operations::Consume as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 4);
    assert_eq!(<kernel_api::abi::call::sign::operations::Attach as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 5);
    assert_eq!(<kernel_api::abi::call::sign::operations::Detach as kernel_api::abi::operation::RegisterCall>::SLOT, kernel_api::abi::PREFIX | (11usize << 32) | 6);
}

#[test]
fn clock_metadata_uses_an_additive_slot_and_two_full_width_values() {
    use kernel_api::abi::{
        call::room::operations::ClockInfo,
        operation::{KernelCall, RegisterCall},
    };

    assert_eq!(ClockInfo::SLOT, kernel_api::abi::PREFIX | 5);
    let clock = ClockInfo::decode_return((0, usize::MAX, 10_000_000)).unwrap();
    assert_eq!(clock.boot_tick(), usize::MAX as u64);
    assert_eq!(clock.hertz().get(), 10_000_000);
    // Older providers report Unknown (-1), so Client can fall back to Clock.
    assert!(ClockInfo::decode_return((usize::MAX, 0, 0)).is_err());
}

#[test]
fn room_inspect_selects_the_ordinary_pool() {
    use kernel_api::abi::{
        call::room::{RoomCall, RoomFail, RoomQuery, operations::Inspect},
        operation::{KernelCall, RegisterCall},
    };

    let request = Inspect {
        query: RoomQuery::Ordinary,
    };
    assert_eq!(Inspect::SLOT, kernel_api::abi::PREFIX | 6);
    assert_eq!(request.pack(), [0; 6]);
    assert_eq!(
        RoomCall::from_wire(Inspect::SLOT, &request.pack()),
        Ok(RoomCall::Inspect {
            query: RoomQuery::Ordinary
        })
    );
    assert!(RoomCall::from_wire(Inspect::SLOT, &[1, 0, 0, 0, 0, 0]).is_err());
    assert_eq!(Inspect::decode_return((2, 0, 0)).unwrap(), 2);
    assert!(Inspect::decode_return((usize::MAX, 0, 0)).is_err());
    assert_eq!(
        Inspect::decode_return(((-4isize) as usize, 0, 0))
            .unwrap_err()
            .source,
        RoomFail::Denied
    );
}
