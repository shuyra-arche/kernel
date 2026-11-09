#[path = "../../src/metal/riscv/console/output.rs"]
mod output;
use kernel_api::address::PAGE_SIZE;
#[repr(align(4096))]
struct Buffer([u8; PAGE_SIZE * 2]);
#[test]
fn virtual_page_boundary_does_not_require_adjacent_physical_pages() {
    let buffer = Buffer([0; PAGE_SIZE * 2]);
    let bytes = &buffer.0[PAGE_SIZE - 3..PAGE_SIZE + 5];
    let base = bytes.as_ptr() as usize;
    let mut calls = Vec::new();
    let written = output::transfer(
        bytes,
        |va, count| match (va - base, count) {
            (0, 3) => Some(0x1ffd),
            (3, 5) => Some(0x9000),
            _ => panic!("noncontiguous physical request"),
        },
        |pa, n| {
            calls.push((pa, n));
            Some(n)
        },
    );
    assert_eq!(written, Some(8));
    assert_eq!(calls, vec![(0x1ffd, 3), (0x9000, 5)]);
}
#[test]
fn backpressure_reports_only_the_accepted_prefix_without_spinning() {
    let buffer = Buffer([0; PAGE_SIZE * 2]);
    let bytes = &buffer.0[PAGE_SIZE - 3..PAGE_SIZE + 5];
    let mut calls = 0;
    assert_eq!(
        output::transfer(
            bytes,
            |va, _| Some(va),
            |_, n| {
                calls += 1;
                Some(if calls == 1 { n } else { 0 })
            }
        ),
        Some(3)
    );
    assert_eq!(calls, 2);
    let mut calls = 0;
    assert_eq!(
        output::transfer(
            &bytes[..3],
            |va, _| Some(va),
            |_, _| {
                calls += 1;
                Some(1)
            }
        ),
        Some(1)
    );
    assert_eq!(calls, 1);
}
#[test]
fn failure_does_not_erase_or_invent_a_previously_written_prefix() {
    let buffer = Buffer([0; PAGE_SIZE * 2]);
    let bytes = &buffer.0[PAGE_SIZE - 3..PAGE_SIZE + 5];
    let mut calls = 0;
    assert_eq!(
        output::transfer(
            bytes,
            |va, _| Some(va),
            |_, n| {
                calls += 1;
                if calls == 1 { Some(n) } else { None }
            }
        ),
        Some(3)
    );
    assert_eq!(
        output::transfer(bytes, |_, _| None, |_, _| panic!("untranslated memory")),
        None
    );
    assert_eq!(
        output::transfer(bytes, |va, _| Some(va), |_, n| Some(n + 1)),
        None
    );
}
