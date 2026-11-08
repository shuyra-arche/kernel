#![feature(allocator_ext)]
extern crate alloc;
#[allow(dead_code)]
#[path = "../../src/machine/switcher/context/extension/mod.rs"]
mod extension;
use extension::{Capability, ExtendedContext};

#[test]
fn first_use_is_single_and_capability_checked() {
    let mut state = ExtendedContext::new();
    let mut status = 0;
    assert_eq!(
        state.enable(
            0xf2000053,
            &mut status,
            Capability {
                float: false,
                vector: None
            }
        ),
        Ok(false)
    );
    assert_eq!(status, 0);
    assert_eq!(
        state.enable(
            0xf2000053,
            &mut status,
            Capability {
                float: true,
                vector: None
            }
        ),
        Ok(true)
    );
    assert_ne!(status & (3 << 13), 0);
    assert!(!state.compatible(Capability {
        float: false,
        vector: None
    }));
    assert!(state.compatible(Capability {
        float: true,
        vector: None
    }));
    assert_eq!(
        state.enable(
            0xf2000053,
            &mut status,
            Capability {
                float: true,
                vector: None
            }
        ),
        Ok(false)
    );
}

#[test]
fn unrelated_and_reserved_instructions_do_not_enable_float() {
    for word in [
        0, 0xffffffff, 0x00100073, 0x00000073, 0xc0001073, 0x00005053, 0x04000053, 0xfff00053,
        0x2002,
    ] {
        assert!(!extension::instruction::float(word), "{word:#x}");
        let mut state = ExtendedContext::new();
        let mut status = 0;
        assert_eq!(
            state.enable(
                word,
                &mut status,
                Capability {
                    float: true,
                    vector: None
                }
            ),
            Ok(false)
        );
        assert_eq!(status, 0);
    }
}

#[test]
fn recognizes_compressed_float_and_control_first_use() {
    for word in [
        0x2000, 0xa000, 0x2082, 0xa002, 0x00302573, 0x00002007, 0x00003007, 0x02000053, 0x02000043,
    ] {
        assert!(extension::instruction::float(word), "{word:#x}");
    }
}

#[test]
fn vector_geometry_is_exact_and_only_supported_shapes_are_admitted() {
    use extension::Shape;
    for bad in [0, 8, 17, 16384] {
        assert!(Shape::checked(bad, 15).is_none());
    }
    for widths in [0, 7, 31] {
        assert!(Shape::checked(32, widths).is_none());
    }
    for bytes in [16, 32, 8192] {
        let shape = Shape::checked(bytes, 15).unwrap();
        let mut state = ExtendedContext::new();
        let mut status = 0;
        assert_eq!(
            state.enable(
                0xc2202573,
                &mut status,
                Capability {
                    float: true,
                    vector: Some(shape)
                }
            ),
            Ok(true)
        );
        assert_ne!(status & (3 << 9), 0);
        assert!(state.compatible(Capability {
            float: true,
            vector: Some(shape)
        }));
        assert!(!state.compatible(Capability {
            float: true,
            vector: None
        }));
        assert!(!state.compatible(Capability {
            float: true,
            vector: Shape::checked(if bytes == 16 { 32 } else { 16 }, 15)
        }));
    }
}

#[test]
fn vector_csr_access_and_absent_hardware_keep_fault_boundaries() {
    for word in [0xc2202573, 0x00802573, 0x00f02573, 0x02000057, 0x00007057] {
        assert!(extension::instruction::vector(word));
        let mut state = ExtendedContext::new();
        let mut status = 0;
        assert_eq!(
            state.enable(
                word,
                &mut status,
                Capability {
                    float: true,
                    vector: None
                }
            ),
            Ok(false)
        );
        assert_eq!(status, 0);
    }
    for word in [
        0xc2201573, 0xc220a573, 0x00b02573, 0x10000007, 0x00100073, 0xffffffff,
    ] {
        assert!(!extension::instruction::vector(word), "{word:#x}");
    }
}
