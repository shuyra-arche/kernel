extern crate alloc;
#[allow(dead_code)]
#[path = "../../src/basis/nest/address/occupancy.rs"]
mod occupancy;
#[allow(dead_code)]
#[path = "../../src/basis/nest/address/placement.rs"]
mod placement;
use kernel_api::address::VirtAddr;
use occupancy::{Occupancy, RegionId};
use placement::Placement;

#[test]
fn address_reuse_requires_exact_generation_and_completed_retirement() {
    let mut occupancy = Occupancy::new();
    let placement = Placement::exact(RegionId::Static, VirtAddr::new(0x1000), 8192).unwrap();
    let (address, generation) = occupancy.reserve(placement).unwrap();
    occupancy.reserve_cuts(2).unwrap();
    let ticket = occupancy.retire(address, address + 4095, generation);
    assert!(occupancy.reserve(placement).is_err());
    assert!(!occupancy.release(address, address + 8191, generation));
    occupancy.reclaim(ticket);
    let first = Placement::exact(RegionId::Static, VirtAddr::new(address), 4096).unwrap();
    let (_, replacement) = occupancy.reserve(first).unwrap();
    assert_ne!(replacement, generation);
    assert!(!occupancy.release(address, address + 4095, generation));
    assert!(occupancy.release(address, address + 4095, replacement));
    assert!(occupancy.has_id(generation));
    assert!(occupancy.release(address + 4096, address + 8191, generation));
    assert!(!occupancy.has_id(generation));
}

#[test]
fn top_page_and_fixed_overlap_use_the_same_occupancy_fact() {
    let mut occupancy = Occupancy::new();
    let top = usize::MAX - 4095;
    let placement = Placement::exact(RegionId::Static, VirtAddr::new(top), 4096).unwrap();
    let (_, generation) = occupancy.reserve(placement).unwrap();
    assert!(occupancy.reserve(placement).is_err());
    assert!(occupancy.release(top, usize::MAX, generation));
    let overflow = Placement::exact(RegionId::Static, VirtAddr::new(top), 8192).unwrap();
    assert!(occupancy.reserve(overflow).is_err());
    occupancy.attach(RegionId::Normal, 0x1000, 0x9fff).unwrap();
    let fixed = Placement::exact(RegionId::Static, VirtAddr::new(0x2000), 4096).unwrap();
    occupancy.reserve(fixed).unwrap();
    assert_eq!(
        occupancy
            .reserve(Placement::lowest(RegionId::Normal, 4096, 8192).unwrap())
            .unwrap()
            .0,
        0x4000
    );
    assert_eq!(
        occupancy
            .reserve(Placement::highest(RegionId::Normal, 4096).unwrap())
            .unwrap()
            .0,
        0x9000
    );
}
