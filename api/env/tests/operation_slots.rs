use kernel_api_env as env;
#[test]
fn every_operation_keeps_its_published_slot() {
    assert_eq!(
        <env::abi::call::chrono::operations::Ticks as env::RegisterCall>::SLOT,
        17179869184u64 as usize
    );
    assert_eq!(
        <env::abi::call::chrono::operations::Clock as env::RegisterCall>::SLOT,
        17179869185u64 as usize
    );
    assert_eq!(
        <env::abi::call::control::operations::Backtrace as env::RegisterCall>::SLOT,
        25769803776u64 as usize
    );
    assert_eq!(
        <env::abi::call::debug::operations::Put as env::RegisterCall>::SLOT,
        34359738368u64 as usize
    );
    assert_eq!(
        <env::abi::call::debug::operations::Get as env::RegisterCall>::SLOT,
        34359738369u64 as usize
    );
    assert_eq!(
        <env::abi::call::debug::operations::SetTrace as env::RegisterCall>::SLOT,
        34359738370u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Unseal as env::RegisterCall>::SLOT,
        21474836480u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Seal as env::RegisterCall>::SLOT,
        21474836481u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Push as env::RegisterCall>::SLOT,
        21474836482u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Pull as env::RegisterCall>::SLOT,
        21474836483u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Peek as env::RegisterCall>::SLOT,
        21474836484u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Wait as env::RegisterCall>::SLOT,
        21474836485u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Ring as env::RegisterCall>::SLOT,
        21474836486u64 as usize
    );
    assert_eq!(
        <env::abi::call::hole::operations::Hush as env::RegisterCall>::SLOT,
        21474836487u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Acquire as env::RegisterCall>::SLOT,
        8589934597u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Release as env::RegisterCall>::SLOT,
        8589934598u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Attach as env::RegisterCall>::SLOT,
        8589934599u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Detach as env::RegisterCall>::SLOT,
        8589934600u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::SetAccess as env::RegisterCall>::SLOT,
        8589934601u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Traverse as env::RegisterCall>::SLOT,
        8589934603u64 as usize
    );
    assert_eq!(
        <env::abi::call::memory::operations::Transfer as env::RegisterCall>::SLOT,
        8589934602u64 as usize
    );
    assert_eq!(
        <env::abi::call::nole::operations::Unseal as env::RegisterCall>::SLOT,
        47244640256u64 as usize
    );
    assert_eq!(
        <env::abi::call::nole::operations::Seal as env::RegisterCall>::SLOT,
        47244640257u64 as usize
    );
    assert_eq!(
        <env::abi::call::nole::operations::Wait as env::RegisterCall>::SLOT,
        47244640258u64 as usize
    );
    assert_eq!(
        <env::abi::call::nole::operations::Ring as env::RegisterCall>::SLOT,
        47244640259u64 as usize
    );
    assert_eq!(
        <env::abi::call::nole::operations::Hush as env::RegisterCall>::SLOT,
        47244640260u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Accord as env::RegisterCall>::SLOT,
        30064771076u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Narrow as env::RegisterCall>::SLOT,
        30064771077u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Revoke as env::RegisterCall>::SLOT,
        30064771078u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Release as env::RegisterCall>::SLOT,
        30064771079u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Inspect as env::RegisterCall>::SLOT,
        30064771080u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Collect as env::RegisterCall>::SLOT,
        30064771081u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Same as env::RegisterCall>::SLOT,
        30064771082u64 as usize
    );
    assert_eq!(
        <env::abi::call::pie::operations::Derive as env::RegisterCall>::SLOT,
        30064771085u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Unseal as env::RegisterCall>::SLOT,
        42949672960u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Seal as env::RegisterCall>::SLOT,
        42949672961u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Open as env::RegisterCall>::SLOT,
        42949672962u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Shut as env::RegisterCall>::SLOT,
        42949672963u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Wait as env::RegisterCall>::SLOT,
        42949672964u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Ring as env::RegisterCall>::SLOT,
        42949672965u64 as usize
    );
    assert_eq!(
        <env::abi::call::pole::operations::Hush as env::RegisterCall>::SLOT,
        42949672966u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Starve as env::RegisterCall>::SLOT,
        0u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Park as env::RegisterCall>::SLOT,
        1u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Reap as env::RegisterCall>::SLOT,
        2u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Wait as env::RegisterCall>::SLOT,
        3u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Wake as env::RegisterCall>::SLOT,
        4u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::Doom as env::RegisterCall>::SLOT,
        5u64 as usize
    );
    assert_eq!(
        <env::abi::call::room::operations::ParkUntil as env::RegisterCall>::SLOT,
        6u64 as usize
    );
    assert_eq!(
        <env::abi::call::tole::operations::Unseal as env::RegisterCall>::SLOT,
        51539607552u64 as usize
    );
    assert_eq!(
        <env::abi::call::tole::operations::Seal as env::RegisterCall>::SLOT,
        51539607553u64 as usize
    );
    assert_eq!(
        <env::abi::call::tole::operations::Attach as env::RegisterCall>::SLOT,
        51539607554u64 as usize
    );
    assert_eq!(
        <env::abi::call::tole::operations::Detach as env::RegisterCall>::SLOT,
        51539607555u64 as usize
    );
    assert_eq!(
        <env::abi::call::tole::operations::Await as env::RegisterCall>::SLOT,
        51539607556u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Build as env::RegisterCall>::SLOT,
        4294967328u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Spawn as env::RegisterCall>::SLOT,
        4294967329u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Embark as env::RegisterCall>::SLOT,
        4294967330u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Debark as env::RegisterCall>::SLOT,
        4294967331u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Slay as env::RegisterCall>::SLOT,
        4294967332u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Join as env::RegisterCall>::SLOT,
        4294967333u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Oust as env::RegisterCall>::SLOT,
        4294967334u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Scan as env::RegisterCall>::SLOT,
        4294967335u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::SelfId as env::RegisterCall>::SLOT,
        4294967336u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Sire as env::RegisterCall>::SLOT,
        4294967337u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Fall as env::RegisterCall>::SLOT,
        4294967338u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Guard as env::RegisterCall>::SLOT,
        4294967339u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Unguard as env::RegisterCall>::SLOT,
        4294967340u64 as usize
    );
    assert_eq!(
        <env::abi::call::unit::operations::Creator as env::RegisterCall>::SLOT,
        4294967341u64 as usize
    );
}
