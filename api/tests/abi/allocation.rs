use kernel_api::abi::register::Wire;
use kernel_api::model::memory::{AcquireSource, Allocation};

#[test]
fn allocation_requires_a_page_and_rounds_only_valid_requests() {
    for size in [0, 1, 4095, usize::MAX - 4094, usize::MAX] {
        assert_eq!(Allocation::new(size, 4096), None);
        // Public fields and handcrafted wire input cannot bypass admission.
        assert_eq!(Allocation { size, align: 4096 }.bytes(), None);
        assert!(AcquireSource::unpack(&[0, size, 4096, 0, 0, 0], &mut 0).is_err());
        assert!(Allocation::unpack(&[size, 4096, 0, 0, 0, 0], &mut 0).is_err());
    }
    for (size, bytes) in [(4096, 4096), (4097, 8192), (12288, 12288)] {
        let allocation = Allocation::new(size, 4096).unwrap();
        assert_eq!(allocation.size, size);
        assert_eq!(allocation.bytes(), Some(bytes));
        assert_eq!(
            AcquireSource::unpack(&[0, size, 4096, 0, 0, 0], &mut 0),
            Ok(AcquireSource::Allocation(allocation))
        );
    }
    assert_eq!(Allocation::new(4096, 3), None);
}
