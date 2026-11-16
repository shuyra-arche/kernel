//! Production active-range arithmetic, isolated from target-only Nest machinery.
#![allow(dead_code)]

#[path = "../../../../src/basis/nest/address/span/access.rs"]
mod access;

use access::{Access, ActiveRange};
use kernel_api::address::VirtAddr;

fn range(start: usize, pages: usize) -> ActiveRange {
    ActiveRange::new(
        VirtAddr::new(start),
        pages * kernel_api::address::PAGE_SIZE,
        Access::READ | Access::WRITE,
        Access::READ | Access::WRITE,
    )
    .unwrap()
}

#[test]
fn split_removes_the_released_middle_and_leaves_both_sides_accessible() {
    let original = [range(0x10000, 4)];
    let released = ActiveRange::split(&original, 0x11000, 0x11fff, None).unwrap();
    assert_eq!(released.len(), 2);
    assert_eq!(released[0].start.as_usize(), 0x10000);
    assert_eq!(released[0].end(), 0x10fff);
    assert_eq!(released[1].start.as_usize(), 0x12000);
    assert_eq!(released[1].end(), 0x13fff);
    assert!(ActiveRange::current_at(&released, 0x11000).is_none());
    assert_eq!(
        ActiveRange::current_at(&released, 0x10fff),
        Some(Access::READ | Access::WRITE)
    );
    assert!(!ActiveRange::cover(&released, 0x11000, 0x11fff));
    assert!(ActiveRange::cover(&released, 0x10000, 0x10fff));
    assert!(ActiveRange::cover(&released, 0x12000, 0x13fff));
}

#[test]
fn splitting_without_overlap_returns_the_original_shape() {
    let original = [range(0x10000, 2)];
    let unchanged = ActiveRange::split(&original, 0x20000, 0x20fff, None).unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0].start.as_usize(), 0x10000);
    assert_eq!(unchanged[0].end(), 0x11fff);
}

#[test]
fn splitting_the_whole_shape_removes_every_active_page() {
    let original = [range(0x30000, 2)];
    let released = ActiveRange::split(&original, 0x30000, 0x31fff, None).unwrap();
    assert!(released.is_empty());
}

#[test]
fn an_edge_split_keeps_only_the_remaining_side() {
    let original = [range(0x40000, 3)];
    let suffix = ActiveRange::split(&original, 0x40000, 0x40fff, None).unwrap();
    assert_eq!(suffix.len(), 1);
    assert_eq!(suffix[0].start.as_usize(), 0x41000);
    assert_eq!(suffix[0].end(), 0x42fff);

    let prefix = ActiveRange::split(&original, 0x42000, 0x42fff, None).unwrap();
    assert_eq!(prefix.len(), 1);
    assert_eq!(prefix[0].start.as_usize(), 0x40000);
    assert_eq!(prefix[0].end(), 0x41fff);
}

#[test]
fn a_gap_between_two_ranges_is_not_covered_or_accessible() {
    let ranges = [range(0x50000, 1), range(0x52000, 1)];
    assert!(!ActiveRange::cover(&ranges, 0x50000, 0x52fff));
    assert!(ActiveRange::current_at(&ranges, 0x51000).is_none());
    assert_eq!(
        ActiveRange::current_at(&ranges, 0x52000),
        Some(Access::READ | Access::WRITE)
    );
}
