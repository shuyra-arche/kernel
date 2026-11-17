//! Exercise the actual production SpanRange checks without target-only MMU state.
#[path = "../../../../src/basis/nest/address/span/geometry.rs"]
mod range;

use kernel_api::address::{PAGE_SIZE, PageCount, PageOffset, VirtAddr};
use kernel_api::model::cell::CellLevel;
use range::SpanRange;

#[test]
fn release_requires_complete_cell_grains_at_fixed_original_coordinates() {
    let grain = 512;
    let base = 0x400000;
    let span = SpanRange::new(
        VirtAddr::new(base),
        2 * grain * PAGE_SIZE,
        CellLevel::new(1).unwrap(),
    )
    .unwrap();
    assert_eq!(span.start(), VirtAddr::new(base));
    assert_eq!(span.size(), 2 * grain * PAGE_SIZE);
    assert_eq!(span.level(), CellLevel::new(1).unwrap());
    assert_eq!(span.last(), base + 2 * grain * PAGE_SIZE - 1);

    assert_eq!(
        span.release_slice(PageOffset::new(0), PageCount::new(grain).unwrap()),
        Ok((base, base + grain * PAGE_SIZE - 1))
    );
    assert_eq!(
        span.release_slice(PageOffset::new(grain), PageCount::new(grain).unwrap(),),
        Ok((base + grain * PAGE_SIZE, base + 2 * grain * PAGE_SIZE - 1))
    );
    for (offset, count) in [(1, grain), (0, 1), (grain - 1, 1), (grain, grain + 1)] {
        assert!(
            span.release_slice(PageOffset::new(offset), PageCount::new(count).unwrap(),)
                .is_err(),
            "releasing a partial Cell must fail"
        );
    }
    assert!(
        span.release_slice(PageOffset::new(2 * grain), PageCount::new(grain).unwrap(),)
            .is_err()
    );
}

#[test]
fn final_address_page_has_a_representable_inclusive_release_end() {
    let top = usize::MAX - (PAGE_SIZE - 1);
    let span = SpanRange::new(VirtAddr::new(top), PAGE_SIZE, CellLevel::BASE).unwrap();

    assert_eq!(
        span.release_slice(PageOffset::new(0), PageCount::new(1).unwrap()),
        Ok((top, usize::MAX))
    );
    assert!(
        span.release_slice(PageOffset::new(1), PageCount::new(1).unwrap())
            .is_err()
    );
}
