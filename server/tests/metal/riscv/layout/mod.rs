#[path = "../../../../src/metal/riscv/layout/plan.rs"]
mod plan;
use plan::*;
#[test]
fn every_supported_geometry_has_one_ordered_layout_at_all_admitted_counts() {
    for bits in [39, 48, 57] {
        for harts in [1, 2, 4, 256] {
            let p = KernelLayout::new(bits, harts, 0x88000000).unwrap();
            assert_eq!(p.normal.last, (1usize << (bits - 1)) - 1);
            assert_eq!(p.direct.base, usize::MAX - p.normal.last);
            let high = [
                p.direct,
                p.resource,
                p.trap,
                p.team,
                p.hart,
                p.reserved,
                p.trampoline,
            ];
            assert!(
                high.windows(2)
                    .all(|pair| pair[0].last.checked_add(1) == Some(pair[1].base))
            );
            assert_eq!(p.trap.last - p.trap.base + 1, 256 * 65536);
            assert_eq!(p.trampoline.last, usize::MAX);
            assert_eq!(p.trampoline.base, TRAMPOLINE);
        }
    }
}
#[test]
fn layout_rejects_unadmitted_geometry_capacity_and_direct_window_collisions() {
    for bits in [0, 38, 40, 64] {
        assert!(KernelLayout::new(bits, 1, 0x88000000).is_none());
    }
    for harts in [0, 257, 4096, usize::MAX] {
        assert!(KernelLayout::new(39, harts, 0x88000000).is_none());
    }
    for bits in [39, 48, 57] {
        for end in [0, 1, usize::MAX, 1usize << (bits - 1)] {
            assert!(KernelLayout::new(bits, 1, end).is_none());
        }
    }
}
