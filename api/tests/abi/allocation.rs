use kernel_api::abi::register::Wire;
use kernel_api::address::{BASE_PAGE_SHIFT, BASE_PAGE_SIZE, PAGE_SHIFT, PAGE_SIZE, PageCount};
use kernel_api::model::cell::{Allocation, CellCreate, CellLevel};

#[test]
fn allocation_requires_a_page_and_rounds_only_valid_requests() {
    for size in [0, 1, 4095, usize::MAX - 4094, usize::MAX] {
        assert_eq!(Allocation::new(size, 4096), None);
        // Public fields and handcrafted wire input cannot bypass admission.
        assert_eq!(
            Allocation {
                size,
                align: 4096,
                level: CellLevel::BASE
            }
            .bytes(),
            None
        );
        let spec = kernel_api::model::cell::CellCreate::Slot(Allocation {
            size,
            align: 4096,
            level: CellLevel::BASE,
        })
        .to_bytes();
        assert!(kernel_api::model::cell::CellCreate::from_bytes(&spec).is_none());
        assert!(Allocation::unpack(&[size, 4096, 0, 0, 0, 0], &mut 0).is_err());
    }
    for (size, bytes) in [(4096, 4096), (4097, 8192), (12288, 12288)] {
        let allocation = Allocation::new(size, 4096).unwrap();
        assert_eq!(allocation.size, size);
        assert_eq!(allocation.bytes(), Some(bytes));
    }
    assert_eq!(Allocation::new(4096, 3), None);
}

#[test]
fn public_range_coordinates_remain_four_kib_even_for_larger_requests() {
    assert_eq!(BASE_PAGE_SHIFT, 12);
    assert_eq!(BASE_PAGE_SIZE, 4096);
    assert_eq!(PAGE_SHIFT, BASE_PAGE_SHIFT);
    assert_eq!(PAGE_SIZE, BASE_PAGE_SIZE);
    assert_eq!(
        PageCount::new(512).unwrap().checked_bytes(),
        Some(2 * 1024 * 1024)
    );
    assert_eq!(PageCount::new(usize::MAX).unwrap().checked_bytes(), None);
    // A large byte request alone does not guarantee physical contiguity.
    assert_eq!(
        Allocation::new(2 * 1024 * 1024, 4096).unwrap().bytes(),
        Some(2 * 1024 * 1024)
    );
}

#[test]
fn levels_roundtrip_and_reject_noncanonical_geometry() {
    for n in 0..=4 {
        let level = CellLevel::new(n).unwrap();
        assert_eq!(level.pages(), Some(1 << (9 * n)));
        assert_eq!(level.bytes(), Some(1 << (12 + 9 * n)));
        let value = Allocation::at_level(level, 2).unwrap();
        assert_eq!(
            CellCreate::from_bytes(&CellCreate::Slot(value).to_bytes()),
            Some(CellCreate::Slot(value))
        );
        let mut words = [0; 6];
        value.pack(&mut words, &mut 0);
        assert_eq!(Allocation::unpack(&words, &mut 0).unwrap(), value);
        assert!(Allocation::at_level(level, 0).is_none());
        assert!(Allocation::at_level(level, usize::MAX).is_none());
        if n > 0 {
            assert_eq!(level.child().unwrap().parent(), Some(level));
            assert!(
                Allocation {
                    size: value.size - 1,
                    ..value
                }
                .bytes()
                .is_none()
            );
            assert!(
                Allocation {
                    align: 4096,
                    ..value
                }
                .bytes()
                .is_none()
            );
        }
    }
    for n in [6, 7, 8, 255, 256, usize::MAX] {
        assert!(CellLevel::new(n).is_none());
    }
}
