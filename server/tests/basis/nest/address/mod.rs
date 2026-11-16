extern crate alloc;

#[allow(dead_code, unused_imports)]
#[path = "../../../../src/basis/name/mod.rs"]
mod name;

mod active;
#[path = "../../name/allocation.rs"]
mod allocation;

mod basis {
    pub(crate) use crate::name;
    pub(crate) mod nest {
        #[allow(dead_code)]
        pub struct Nest;
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum NestError {
            OutOfMemory,
            NotAligned,
            NoRegion,
        }
    }
}

mod metal {
    pub(crate) mod generic {
        pub(crate) mod mmu {
            pub(crate) mod internal {
                #[derive(Clone, Copy)]
                pub(crate) struct Attributes(pub(crate) u64);
                impl Attributes {
                    pub(crate) fn from_bits_truncate(bits: u64) -> Self {
                        Self(bits)
                    }
                    pub(crate) fn bits(self) -> u64 {
                        self.0
                    }
                }
            }
        }
    }
    pub(crate) mod mmu {
        pub(crate) mod internal {
            pub(crate) fn canonical(address: usize) -> kernel_api::address::VirtAddr {
                kernel_api::address::VirtAddr::new(address)
            }
        }
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

#[test]
fn gap_selection_uses_no_scratch_allocation_and_keeps_retired_ranges() {
    let mut layout = Layout::new();
    layout
        .register_window(RegionKind::Normal, 0x1000, 0x9fff)
        .unwrap();
    let (_, id) = layout
        .reserve(Placement::fixed(VirtAddr::new(0x3000), 8192).unwrap())
        .unwrap();
    let retired = layout
        .occupancy
        .prepare_retirement(0x3000, 0x4fff, id)
        .unwrap();
    layout.occupancy.retire(retired);
    // Occupancy already has spare insertion capacity. A Vec of gaps would fail.
    let low = allocation::fail_after(0, || {
        layout.reserve(Placement::lowest(RegionKind::Normal, 4096, 8192).unwrap())
    })
    .unwrap();
    assert_eq!(low.0, 0x2000);
    assert!(layout.occupancy.release(low.0, low.0 + 4095, low.1));
    let high = allocation::fail_after(0, || {
        layout.reserve(
            Placement::new(
                RegionKind::Normal,
                4096,
                8192,
                address::PlacementAt::Highest,
            )
            .unwrap(),
        )
    })
    .unwrap();
    assert_eq!(high.0, 0x8000);
    let mut full = Layout::new();
    full.register_window(RegionKind::Normal, 0x1000, 0x1fff)
        .unwrap();
    assert_eq!(
        allocation::fail_after(0, || full
            .reserve(Placement::highest(RegionKind::Normal, 4096).unwrap())),
        Err(address::AddressError::OutOfMemory)
    );
    assert_eq!(
        full.reserve(Placement::highest(RegionKind::Normal, 4096).unwrap())
            .unwrap()
            .0,
        0x1000
    );
}

#[test]
fn partial_release_keeps_generation_on_both_sides_and_frees_a_reusable_hole() {
    let mut layout = Layout::new();
    let page = 4096;
    let placement = Placement::fixed(VirtAddr::new(0x10000), 3 * page).unwrap();
    let (start, generation) = layout.reserve(placement).unwrap();
    assert!(
        layout
            .occupancy
            .release_range(start + page, start + 2 * page - 1, generation)
    );
    assert!(layout.occupancy.has_id(generation));

    let hole = Placement::fixed(VirtAddr::new(start + page), page).unwrap();
    let (middle, replacement) = layout.reserve(hole).unwrap();
    assert_eq!(middle, start + page);
    assert_ne!(replacement, generation);
    assert!(
        layout
            .reserve(Placement::fixed(VirtAddr::new(start), page).unwrap())
            .is_err()
    );
    assert!(
        layout
            .reserve(Placement::fixed(VirtAddr::new(start + 2 * page), page).unwrap())
            .is_err()
    );

    assert!(
        layout
            .occupancy
            .release_range(start, start + page - 1, generation)
    );
    assert!(
        layout
            .occupancy
            .release_range(start + 2 * page, start + 3 * page - 1, generation)
    );
    assert!(!layout.occupancy.has_id(generation));
    assert!(layout.occupancy.has_id(replacement));
    assert!(
        layout
            .occupancy
            .release(middle, middle + page - 1, replacement)
    );
}

#[test]
fn partial_release_fails_closed_when_the_generation_does_not_own_every_page() {
    let mut layout = Layout::new();
    let page = 4096;
    let left = Placement::fixed(VirtAddr::new(0x20000), page).unwrap();
    let right = Placement::fixed(VirtAddr::new(0x21000), page).unwrap();
    let (left_address, left_generation) = layout.reserve(left).unwrap();
    let (right_address, right_generation) = layout.reserve(right).unwrap();
    assert_eq!(left_address, 0x20000);
    assert_eq!(right_address, 0x21000);

    assert!(!layout.occupancy.release_range(
        left_address,
        right_address + page - 1,
        left_generation,
    ));
    assert!(layout.occupancy.has_id(left_generation));
    assert!(layout.occupancy.has_id(right_generation));
    assert!(
        !layout
            .occupancy
            .release_range(left_address, right_address, left_generation)
    );
    assert!(layout.occupancy.has_id(left_generation));
    assert!(
        layout
            .occupancy
            .release_range(left_address, left_address + page - 1, left_generation)
    );
    assert!(!layout.occupancy.has_id(left_generation));
    assert!(
        layout
            .occupancy
            .release(right_address, right_address + page - 1, right_generation)
    );
}

#[test]
fn partial_release_waits_for_the_retirement_ticket_to_settle() {
    let mut layout = Layout::new();
    let page = 4096;
    let placement = Placement::fixed(VirtAddr::new(0x30000), page).unwrap();
    let (start, generation) = layout.reserve(placement).unwrap();
    let prepared = layout
        .occupancy
        .prepare_retirement(start, start + page - 1, generation)
        .unwrap();
    let ticket = layout.occupancy.retire(prepared)[0].2;
    assert!(
        !layout
            .occupancy
            .release_range(start, start + page - 1, generation)
    );
    assert!(layout.occupancy.has_id(generation));
    layout.occupancy.reclaim(ticket);
    assert!(!layout.occupancy.has_id(generation));
    let (reused, replacement) = layout.reserve(placement).unwrap();
    assert_eq!(reused, start);
    assert_ne!(replacement, generation);
}

#[test]
fn partial_release_rejects_unaligned_windows() {
    let mut layout = Layout::new();
    let page = 4096;
    let placement = Placement::fixed(VirtAddr::new(0x60000), page).unwrap();
    let (start, generation) = layout.reserve(placement).unwrap();
    assert!(
        !layout
            .occupancy
            .release_range(start + 1, start + page - 1, generation)
    );
    assert!(
        !layout
            .occupancy
            .release_range(start, start + page - 2, generation)
    );
    assert!(layout.occupancy.has_id(generation));
    assert!(
        layout
            .occupancy
            .release_range(start, start + page - 1, generation)
    );
    assert!(!layout.occupancy.has_id(generation));
}
