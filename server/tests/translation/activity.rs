use super::{entry::PageTableEntry, flags::PteFlags};
use core::mem::{align_of, size_of};
use std::sync::{
    Barrier,
    atomic::{AtomicU64, Ordering},
};

// Model the hardware participant's full-width atomic updates, without adding
// a hardware-write method to the production PTE interface.
fn hardware(entry: &PageTableEntry) -> &AtomicU64 {
    assert_eq!(size_of::<PageTableEntry>(), 8);
    assert_eq!(align_of::<PageTableEntry>(), 8);
    // SAFETY: repr(transparent) wraps exactly one AtomicU64; both views use only
    // atomic shared accesses, and the reference cannot outlive its storage.
    unsafe { &*(entry as *const PageTableEntry).cast::<AtomicU64>() }
}

#[test]
fn permission_changes_preserve_hardware_evidence_and_mapping_identity() {
    let entry = PageTableEntry::default();
    entry.set(
        0x12345,
        PteFlags::V | PteFlags::R | PteFlags::W | PteFlags::U,
    );
    let before = entry.read();
    hardware(&entry).fetch_or((PteFlags::A | PteFlags::D).bits(), Ordering::AcqRel);
    // This proposal was computed before the hardware updated A/D.
    entry.set_flags((before.flags() - PteFlags::W) | PteFlags::X);
    let after = entry.read();
    assert_eq!(after.paddr(), before.paddr());
    assert!(after.flags().contains(
        PteFlags::A | PteFlags::D | PteFlags::R | PteFlags::X | PteFlags::U | PteFlags::V
    ));
    assert!(!after.flags().contains(PteFlags::W));
    entry.clear();
    assert!(!entry.read().is_valid());
    entry.set(0xabc, PteFlags::V);
    assert!(entry.read().is_branch());
    assert_eq!(entry.read().paddr(), 0xabc000);
    assert!(!entry.read().flags().intersects(PteFlags::A | PteFlags::D));
}

#[test]
fn hardware_update_racing_permission_commit_is_never_erased() {
    for _ in 0..256 {
        let entry = PageTableEntry::default();
        entry.set(0x103, PteFlags::V | PteFlags::R | PteFlags::W);
        let start = Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                hardware(&entry).fetch_or((PteFlags::A | PteFlags::D).bits(), Ordering::AcqRel);
            });
            start.wait();
            entry.set_flags(PteFlags::V | PteFlags::R);
        });
        let result = entry.read();
        assert_eq!(result.paddr(), 0x103000);
        assert!(result.flags().contains(PteFlags::A | PteFlags::D));
        assert!(!result.flags().contains(PteFlags::W));
    }
}

#[test]
fn walk_observes_one_complete_pte_word() {
    let entry = PageTableEntry::default();
    let a = (0x123u64 << 10) | (PteFlags::V | PteFlags::X).bits();
    let b = (0xabcu64 << 10) | (PteFlags::V | PteFlags::R | PteFlags::W).bits();
    hardware(&entry).store(a, Ordering::Release);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..10_000 {
                hardware(&entry).store(b, Ordering::Release);
                hardware(&entry).store(a, Ordering::Release);
            }
        });
        for _ in 0..10_000 {
            let value = entry.read();
            match value.paddr() {
                0x123000 => assert_eq!(value.flags().bits(), (PteFlags::V | PteFlags::X).bits()),
                0xabc000 => assert_eq!(
                    value.flags().bits(),
                    (PteFlags::V | PteFlags::R | PteFlags::W).bits()
                ),
                other => panic!("torn PTE address: {other:#x}"),
            }
        }
    });
}
