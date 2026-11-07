extern crate alloc;
#[allow(dead_code)]
#[path = "../../src/basis/zone/mod.rs"]
mod zone;
mod basis {
    pub(crate) use crate::zone;
}
use kernel_api::address::VirtAddr;
use zone::{Placement, RegionId, Zone};

#[test]
fn address_reuse_requires_exact_generation_and_completed_retirement() {
    let mut zone = Zone::<()>::new();
    let placement = Placement::exact(RegionId::Static, VirtAddr::new(0x1000), 8192).unwrap();
    let (address, generation) = zone.reserve(placement).unwrap();
    zone.prepare_cleanup(generation, || Ok(())).unwrap();
    zone.reserve_cuts(2).unwrap();
    let ticket = zone.retire(address, address + 4095, generation);
    assert!(zone.reserve(placement).is_err());
    assert!(!zone.release(address, address + 8191, generation));
    zone.reclaim(ticket);
    let first = Placement::exact(RegionId::Static, VirtAddr::new(address), 4096).unwrap();
    let (_, replacement) = zone.reserve(first).unwrap();
    assert_ne!(replacement, generation);
    assert!(!zone.release(address, address + 4095, generation));
    assert!(zone.release(address, address + 4095, replacement));
    assert!(zone.has_id(generation));
    assert!(zone.release(address + 4096, address + 8191, generation));
    assert!(!zone.has_id(generation));
}

#[test]
fn top_page_and_fixed_overlap_use_the_same_occupancy_fact() {
    let mut zone = Zone::<()>::new();
    let top = usize::MAX - 4095;
    let placement = Placement::exact(RegionId::Static, VirtAddr::new(top), 4096).unwrap();
    let (_, generation) = zone.reserve(placement).unwrap();
    assert!(zone.reserve(placement).is_err());
    assert!(zone.release(top, usize::MAX, generation));
    let overflow = Placement::exact(RegionId::Static, VirtAddr::new(top), 8192).unwrap();
    assert!(zone.reserve(overflow).is_err());
    zone.attach(RegionId::Normal, 0x1000, 0x9fff).unwrap();
    let fixed = Placement::exact(RegionId::Static, VirtAddr::new(0x2000), 4096).unwrap();
    zone.reserve(fixed).unwrap();
    assert_eq!(
        zone.reserve(Placement::lowest(RegionId::Normal, 4096, 8192).unwrap())
            .unwrap()
            .0,
        0x4000
    );
    assert_eq!(
        zone.reserve(Placement::highest(RegionId::Normal, 4096).unwrap())
            .unwrap()
            .0,
        0x9000
    );
}
