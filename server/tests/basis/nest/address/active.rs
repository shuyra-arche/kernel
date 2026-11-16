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
