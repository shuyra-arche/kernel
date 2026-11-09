//! A second provider can admit its physical input without an RV64 CPU or PLIC.
//! This is a source-contract fixture, not a second-architecture boot test.
use kernel_api::boot::platform::{Memory, Region, Resources};

#[test]
fn opaque_metadata_and_mmio_custody_do_not_require_an_isa() {
    let metadata = Region::new(0x40020000, 73);
    let image = Region::new(0x40000000, 0x10000);
    let memory = Memory::new(
        &[Region::new(0x40000000, 0x100000)],
        &[],
        &[],
        metadata,
        image,
        None,
    )
    .unwrap();
    assert!(memory.protects(image));
    assert_eq!(memory.metadata(), Region::new(metadata.base, 4096));
    assert!(
        !memory
            .free()
            .iter()
            .any(|r| r.range().contains(&metadata.base))
    );

    // These are physical reservations, not invented PLIC contexts or Sv modes.
    let control = [Region::new(0x10000000, 0x10000)];
    let external = [Region::new(0x20000000, 0x10000)];
    let ordinary = [Region::new(0x30000000, 0x1000)];
    let resources = Resources {
        authorities: &ordinary,
        protected: &control,
        external: &external,
    };
    resources.validate(&memory).unwrap();
    assert!(
        resources
            .admits_external(Region::new(0x20001000, 4096))
            .is_ok()
    );
    assert!(resources.admits_external(control[0]).is_err());

    // Common validation still withholds RAM and aliases of protected pages.
    for invalid in [
        Region::new(0x40080000, 4096),
        Region::new(0x10000001, 1),
        Region::new(0x20000001, 1),
        Region::new(usize::MAX - 3, 8),
    ] {
        assert!(
            Resources {
                authorities: &[invalid],
                ..resources
            }
            .validate(&memory)
            .is_err()
        );
    }
    assert!(
        Memory::new(
            &[Region::new(0x40000000, 0x100000)],
            &[],
            &[],
            image,
            image,
            None,
        )
        .is_err()
    );
}
