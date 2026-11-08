extern crate alloc;

use kernel_api::boot::platform::{Memory, Region};
#[allow(dead_code)]
#[path = "../../src/metal/allocator/error.rs"]
mod error;
#[path = "../../src/metal/allocator/frame/inner.rs"]
mod frame;
#[path = "../../src/metal/allocator/link.rs"]
mod link;
mod metal {
    pub mod board {
        pub struct Machine {
            pub memory: crate::Memory,
        }
        pub static FACTS: std::sync::OnceLock<Machine> = std::sync::OnceLock::new();
        pub fn info() -> &'static Machine {
            FACTS.get().unwrap()
        }
    }
    pub mod allocator {
        pub(crate) use crate::{error::InitError, link::Link};
        pub mod bump {
            pub fn frontier() -> usize {
                super::super::board::info().memory.bootstrap().base + 8192
            }
        }
        pub mod statistics {
            pub fn install_frame_kinds(_: usize) -> Result<(), super::InitError> {
                Ok(())
            }
        }
    }
    pub mod console {
        pub fn _write(args: core::fmt::Arguments) {
            eprintln!("{args}")
        }
    }
}

#[test]
fn frame_split_and_merge_never_publish_reserved_pages_or_bank_holes() {
    // Real writable backing, with a canary across every byte the allocator must
    // not touch. Only platform/diagnostic hooks are replaced in this host test.
    let layout = std::alloc::Layout::from_size_align(0x200000, 0x10000).unwrap();
    let ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!ptr.is_null());
    let base = ptr as usize;
    unsafe {
        ptr.write_bytes(0xa5, layout.size());
    }
    let memory = Memory::new(
        &[
            Region::new(base, 0x80000),
            Region::new(base + 0x100000, 0x80000),
        ],
        &[Region::new(base + 0x40000, 0x10000)],
        &[],
        Region::new(base + 0x20000, 4096),
        Region::new(base, 0x10000),
        None,
    )
    .unwrap();
    metal::board::FACTS
        .set(metal::board::Machine { memory })
        .unwrap_or_else(|_| panic!("once"));
    let m = &metal::board::info().memory;
    let usable = |addr: usize| {
        m.free()
            .iter()
            .any(|r| r.base <= addr && addr + 4096 <= r.range().end)
            && !(m.bootstrap().base <= addr && addr < metal::allocator::bump::frontier())
    };
    let expected = (0..layout.size())
        .step_by(4096)
        .filter(|offset| usable(base + offset))
        .count();
    let mut allocator = frame::FrameInner::new();
    allocator.init().unwrap();
    let mut indices = Vec::new();
    while let Some(index) = unsafe { allocator.split_block(0) } {
        assert!(usable(allocator.frame_addr(index)));
        indices.push(index);
        assert!(indices.len() <= expected);
    }
    assert_eq!(indices.len(), expected);
    indices.sort_unstable();
    assert!(indices.windows(2).all(|p| p[0] != p[1]));
    // Free in two passes to force both directions of buddy coalescing.
    for parity in 0..2 {
        for index in indices.iter().skip(parity).step_by(2) {
            unsafe {
                allocator.merge_block(*index, 0);
            }
        }
    }
    let mut large = Vec::new();
    while let Some(index) = unsafe { allocator.split_block(4) } {
        let addr = allocator.frame_addr(index);
        assert_eq!(addr % 0x10000, 0);
        for offset in (0..0x10000).step_by(4096) {
            assert!(usable(addr + offset));
        }
        large.push(index);
    }
    assert!(!large.is_empty());
    for index in large {
        unsafe {
            allocator.merge_block(index, 4);
        }
    }
    for offset in (0..layout.size())
        .step_by(4096)
        .filter(|offset| !usable(base + offset))
    {
        let bytes = unsafe { std::slice::from_raw_parts(ptr.add(offset), 4096) };
        assert!(
            bytes.iter().all(|byte| *byte == 0xa5),
            "protected page {offset:#x} changed"
        );
    }
    drop(allocator);
    unsafe {
        std::alloc::dealloc(ptr, layout);
    }
}

mod entry {
    pub(crate) mod boot {
        pub(crate) use crate::metal::board as facts;
    }
}
