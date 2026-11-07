#[test]
fn every_operation_keeps_its_published_slot() {
    assert_eq!(
        <kernel_api::abi::call::chrono::operations::Ticks as kernel_api::abi::operation::RegisterCall>::SLOT,
        17179869184u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::chrono::operations::Clock as kernel_api::abi::operation::RegisterCall>::SLOT,
        17179869185u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::control::operations::Backtrace as kernel_api::abi::operation::RegisterCall>::SLOT,
        25769803776u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::debug::operations::Put as kernel_api::abi::operation::RegisterCall>::SLOT,
        34359738368u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::debug::operations::Get as kernel_api::abi::operation::RegisterCall>::SLOT,
        34359738369u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::debug::operations::SetTrace as kernel_api::abi::operation::RegisterCall>::SLOT,
        34359738370u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836480u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836481u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Push as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836482u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Pull as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836483u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Peek as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836484u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::clone_view::operations::Wait as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836485u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Acquire as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934597u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Release as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934598u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Attach as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934599u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Detach as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934600u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::SetAccess as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934601u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Traverse as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934603u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::memory::operations::Transfer as kernel_api::abi::operation::RegisterCall>::SLOT,
        8589934602u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Accord as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771076u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Narrow as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771077u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Revoke as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771078u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Release as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771079u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Inspect as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771080u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Collect as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771081u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Same as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771082u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pend::operations::Derive as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771085u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672960u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672961u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Open as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672962u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::share_view::operations::Shut as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672963u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Starve as kernel_api::abi::operation::RegisterCall>::SLOT,
        0u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Park as kernel_api::abi::operation::RegisterCall>::SLOT,
        1u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Reap as kernel_api::abi::operation::RegisterCall>::SLOT,
        2u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Wait as kernel_api::abi::operation::RegisterCall>::SLOT,
        3u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Wake as kernel_api::abi::operation::RegisterCall>::SLOT,
        4u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::Doom as kernel_api::abi::operation::RegisterCall>::SLOT,
        5u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::room::operations::ParkUntil as kernel_api::abi::operation::RegisterCall>::SLOT,
        6u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Build as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967328u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Spawn as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967329u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Embark as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967330u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Debark as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967331u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Slay as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967332u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Join as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967333u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Oust as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967334u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Scan as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967335u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::SelfId as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967336u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Sire as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967337u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Fall as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967338u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Guard as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967339u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Unguard as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967340u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::unit::operations::Creator as kernel_api::abi::operation::RegisterCall>::SLOT,
        4294967341u64 as usize
    );
    assert_eq!(<kernel_api::abi::call::sign::operations::Create as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 0);
    assert_eq!(<kernel_api::abi::call::sign::operations::Close as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 1);
    assert_eq!(<kernel_api::abi::call::sign::operations::Listen as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 2);
    assert_eq!(<kernel_api::abi::call::sign::operations::Trigger as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 3);
    assert_eq!(<kernel_api::abi::call::sign::operations::Consume as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 4);
    assert_eq!(<kernel_api::abi::call::sign::operations::Route as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 5);
    assert_eq!(<kernel_api::abi::call::sign::operations::Unroute as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 6);
    assert_eq!(<kernel_api::abi::call::sign::operations::Delegate as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 7);
    assert_eq!(<kernel_api::abi::call::sign::operations::Release as kernel_api::abi::operation::RegisterCall>::SLOT, (11usize << 32) | 8);
}
