extern crate alloc;

#[allow(dead_code, unused_imports)]
#[path = "../../src/basis/name/mod.rs"]
mod name;

#[path = "../name/allocation.rs"]
mod allocation;

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod nest {
        #[allow(dead_code)]
        pub struct Nest;
    }
}

#[allow(dead_code, unused_imports)]
#[path = "support.rs"]
mod address;

use address::{Layout, Placement, RegionKind};
use kernel_api::address::VirtAddr;

#[test]
fn address_reuse_requires_exact_generation_and_completed_retirement() {
    let mut layout = Layout::new();
    let placement = Placement::fixed(VirtAddr::new(0x1000), 8192).unwrap();
    let (address, generation) = layout.reserve(placement).unwrap();
    let prepared = layout
        .occupancy
        .prepare_retirement(address, address + 4095, generation)
        .unwrap();
    let ticket = layout.occupancy.retire(prepared)[0].2;
    assert_eq!(
        layout.reserve(placement),
        Err(address::AddressError::Occupied)
    );
    assert!(
        !layout
            .occupancy
            .release(address, address + 8191, generation)
    );
    layout.occupancy.reclaim(ticket);
    let first = Placement::fixed(VirtAddr::new(address), 4096).unwrap();
    let (_, replacement) = layout.reserve(first).unwrap();
    assert_ne!(replacement, generation);
    assert!(
        !layout
            .occupancy
            .release(address, address + 4095, generation)
    );
    assert!(
        layout
            .occupancy
            .release(address, address + 4095, replacement)
    );
    assert!(layout.occupancy.has_id(generation));
    assert!(
        layout
            .occupancy
            .release(address + 4096, address + 8191, generation)
    );
    assert!(!layout.occupancy.has_id(generation));
}

#[test]
fn segmented_retirement_prepares_all_tickets_before_changing_occupancy() {
    let mut layout = Layout::new();
    let placement = Placement::fixed(VirtAddr::new(0x1000), 5 * 4096).unwrap();
    let (start, generation) = layout.reserve(placement).unwrap();
    let prepared = layout
        .occupancy
        .prepare_retirement(0x3000, 0x3fff, generation)
        .unwrap();
    let middle = layout.occupancy.retire(prepared)[0].2;
    let before = layout.occupancy.pieces(start, 0x5fff, generation).unwrap();
    assert_eq!(
        before,
        [
            (0x1000, 0x2fff, 0),
            (0x3000, 0x3fff, middle),
            (0x4000, 0x5fff, 0)
        ]
    );

    // Failure after allocating the pieces snapshot must leave every span and
    // old ticket unchanged. No occupancy or PTE commit has taken place.
    assert!(
        allocation::fail_after(1, || layout
            .occupancy
            .prepare_retirement(start, 0x5fff, generation))
        .is_err()
    );
    assert_eq!(
        layout.occupancy.pieces(start, 0x5fff, generation).unwrap(),
        before
    );
    let prepared = layout
        .occupancy
        .prepare_retirement(start, 0x5fff, generation)
        .unwrap();
    drop(prepared); // Later PTE preparation may fail: abandoning is harmless.
    assert_eq!(
        layout.occupancy.pieces(start, 0x5fff, generation).unwrap(),
        before
    );

    let prepared = layout
        .occupancy
        .prepare_retirement(start, 0x5fff, generation)
        .unwrap();
    let committed = allocation::fail_after(0, || layout.occupancy.retire(prepared));
    assert_eq!(committed[1].2, middle);
    assert!(committed[0].2 != 0 && committed[2].2 != 0 && committed[0].2 != committed[2].2);
    assert_eq!(
        layout.reserve(placement),
        Err(address::AddressError::Occupied)
    );
    layout.occupancy.reclaim(middle);
    assert_eq!(
        layout.reserve(placement),
        Err(address::AddressError::Occupied)
    );
    layout.occupancy.reclaim(committed[0].2);
    layout.occupancy.reclaim(committed[2].2);
    assert!(layout.reserve(placement).is_ok());
}

#[test]
fn top_page_and_fixed_overlap_use_the_same_occupancy_fact() {
    let mut layout = Layout::new();
    let top = usize::MAX - 4095;
    let placement = Placement::fixed(VirtAddr::new(top), 4096).unwrap();
    let (_, generation) = layout.reserve(placement).unwrap();
    assert_eq!(
        layout.reserve(placement),
        Err(address::AddressError::Occupied)
    );
    assert!(layout.occupancy.release(top, usize::MAX, generation));
    let overflow = Placement::fixed(VirtAddr::new(top), 8192).unwrap();
    assert!(layout.reserve(overflow).is_err());
    layout
        .register_window(RegionKind::Normal, 0x1000, 0x9fff)
        .unwrap();
    let fixed = Placement::fixed(VirtAddr::new(0x2000), 4096).unwrap();
    layout.reserve(fixed).unwrap();
    assert_eq!(
        layout
            .reserve(Placement::lowest(RegionKind::Normal, 4096, 8192).unwrap())
            .unwrap()
            .0,
        0x4000
    );
    assert_eq!(
        layout
            .reserve(Placement::highest(RegionKind::Normal, 4096).unwrap())
            .unwrap()
            .0,
        0x9000
    );
}
