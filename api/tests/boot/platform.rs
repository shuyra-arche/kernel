use kernel_api::boot::platform::{
    Cpu, FirmwareId, Harts, Memory, Paging, Region,
    devices::{Devices, Plic},
};
fn memory() -> Memory {
    Memory::new(
        &[Region::new(0x80000000, 0x100000)],
        &[Region::new(0x80080000, 4096)],
        &[Region::new(0x80090000, 4096)],
        Region::new(0x80020000, 4096),
        Region::new(0x80000000, 0x10000),
        None,
    )
    .unwrap()
}
#[test]
fn raw_platform_facts_cannot_publish_protected_memory() {
    let m = memory();
    for page in [0x80000000, 0x80020000, 0x80080000, 0x80090000] {
        assert!(!m.free().iter().any(|r| r.range().contains(&page)));
    }
    assert!(!m.contains_identity(Region::new(0x80090000, 4096)));
    assert!(!m.protects(Region::new(0x80000000, 0x20000)));
    let protection = [
        Region::new(0x3000000, 4096),
        Region::new(0xc000000, 0x400000),
    ];
    let contexts = [1usize];
    let irq = Plic {
        base: 0xc000000,
        size: 0x400000,
        sources: 31,
        contexts: &contexts,
        selected: 1,
        queues: [1, 2, 3],
    };
    let mut d = Devices {
        authorities: &[],
        protected: &protection,
        external: &[Region::new(0x40000000, 0x10000)],
        controllers: &protection[..1],
        interrupt: Some(irq),
    };
    d.validate(&m).unwrap();
    assert!(d.admits_external(Region::new(0x40001000, 4096)).is_ok());
    assert!(d.admits_external(protection[0]).is_err());
    let alias = [Region::new(0x3000001, 4)];
    d.authorities = &alias;
    assert!(d.validate(&m).is_err());
    d.authorities = &protection[..1];
    assert!(d.validate(&m).is_err());
    d.authorities = &[];
    d.interrupt = Some(Plic {
        contexts: &[usize::MAX],
        selected: usize::MAX,
        ..irq
    });
    assert!(d.validate(&m).is_err());
    d.interrupt = Some(Plic {
        queues: [1, 1, 3],
        ..irq
    });
    assert!(d.validate(&m).is_err());
    d.interrupt = None;
    assert!(d.validate(&m).is_err());
}
#[test]
fn cpu_identity_admission_is_bounded_and_boot_slot_is_explicit() {
    let cpu = Cpu {
        id: FirmwareId::new(7),
        paging: Paging::Sv48,
        floating: true,
        vector: false,
        sstc: false,
    };
    assert!(Harts::new(&[cpu, cpu], cpu.id, 100).is_err());
    assert!(Harts::new(&[cpu], FirmwareId::new(0), 100).is_err());
    assert!(Harts::new(&[cpu], cpu.id, 0).is_err());
    let other = Cpu {
        id: FirmwareId::new(300),
        paging: Paging::Sv39,
        ..cpu
    };
    let h = Harts::new(&[cpu, other], cpu.id, 100).unwrap();
    assert_eq!(h.paging(), Paging::Sv39);
    assert_eq!(
        h.firmware(kernel_api::boot::platform::HartId::new(1)).get(),
        300
    );
}
