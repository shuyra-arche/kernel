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
        <kernel_api::abi::call::hole::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836480u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836481u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Push as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836482u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Pull as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836483u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Peek as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836484u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Wait as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836485u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Ring as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836486u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::hole::operations::Hush as kernel_api::abi::operation::RegisterCall>::SLOT,
        21474836487u64 as usize
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
        <kernel_api::abi::call::nole::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        47244640256u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::nole::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        47244640257u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::nole::operations::Wait as kernel_api::abi::operation::RegisterCall>::SLOT,
        47244640258u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::nole::operations::Ring as kernel_api::abi::operation::RegisterCall>::SLOT,
        47244640259u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::nole::operations::Hush as kernel_api::abi::operation::RegisterCall>::SLOT,
        47244640260u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Accord as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771076u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Narrow as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771077u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Revoke as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771078u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Release as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771079u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Inspect as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771080u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Collect as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771081u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Same as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771082u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pie::operations::Derive as kernel_api::abi::operation::RegisterCall>::SLOT,
        30064771085u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672960u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672961u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Open as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672962u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Shut as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672963u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Wait as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672964u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Ring as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672965u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::pole::operations::Hush as kernel_api::abi::operation::RegisterCall>::SLOT,
        42949672966u64 as usize
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
        <kernel_api::abi::call::tole::operations::Unseal as kernel_api::abi::operation::RegisterCall>::SLOT,
        51539607552u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::tole::operations::Seal as kernel_api::abi::operation::RegisterCall>::SLOT,
        51539607553u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::tole::operations::Attach as kernel_api::abi::operation::RegisterCall>::SLOT,
        51539607554u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::tole::operations::Detach as kernel_api::abi::operation::RegisterCall>::SLOT,
        51539607555u64 as usize
    );
    assert_eq!(
        <kernel_api::abi::call::tole::operations::Await as kernel_api::abi::operation::RegisterCall>::SLOT,
        51539607556u64 as usize
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
}
