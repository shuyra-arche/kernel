#![feature(allocator_ext)]
extern crate alloc;
#[path = "../../src/machine/switcher/context/extension/mod.rs"]
mod extension;
use extension::{Capability, ExtendedContext};

#[test]
fn first_use_is_single_and_capability_checked() {
    let mut state = ExtendedContext::new();
    let mut status = 0;
    assert_eq!(
        state.enable(0xf2000053, &mut status, Capability { float: false }),
        Ok(false)
    );
    assert_eq!(status, 0);
    assert_eq!(
        state.enable(0xf2000053, &mut status, Capability { float: true }),
        Ok(true)
    );
    assert_ne!(status & (3 << 13), 0);
    assert!(!state.compatible(Capability { float: false }));
    assert!(state.compatible(Capability { float: true }));
    assert_eq!(
        state.enable(0xf2000053, &mut status, Capability { float: true }),
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
            state.enable(word, &mut status, Capability { float: true }),
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
