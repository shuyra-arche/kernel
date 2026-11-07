use crate::device::{
    Phase,
    pci::{Config, Endpoint, Error},
};
use crate::iommu::retirement::Owner;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Pci(Arc<Mutex<Registers>>);
struct Registers {
    words: [u32; 1024],
    writes: Vec<(usize, u32)>,
    stuck: bool,
}
impl Pci {
    fn new() -> Self {
        let mut words = [0; 1024];
        words[0] = 0x11e81234;
        words[1] = 1 << 20;
        words[0x34 / 4] = 0x40;
        words[0x40 / 4] = (0x81 << 16) | 5;
        Self(Arc::new(Mutex::new(Registers {
            words,
            writes: Vec::new(),
            stuck: false,
        })))
    }
    fn set(&self, offset: usize, value: u32) {
        self.0.lock().unwrap().words[offset / 4] = value;
    }
}
unsafe impl Config for Pci {
    fn read32(&self, offset: usize) -> u32 {
        self.0.lock().unwrap().words[offset / 4]
    }
    fn write16(&mut self, offset: usize, value: u16) {
        let mut r = self.0.lock().unwrap();
        r.writes.push((offset, value as u32));
        if r.stuck && offset == 4 {
            return;
        }
        let shift = (offset % 4) * 8;
        r.words[offset / 4] =
            (r.words[offset / 4] & !(0xffff << shift)) | (u32::from(value) << shift);
    }
    fn write32(&mut self, offset: usize, value: u32) {
        let mut r = self.0.lock().unwrap();
        r.writes.push((offset, value));
        r.words[offset / 4] = if value == u32::MAX && (0x10..0x28).contains(&offset) {
            if offset == 0x10 { 0xfff00000 } else { 0 }
        } else {
            value
        };
    }
    fn sync(&self) {}
}

#[test]
fn ownership_config_and_bar_boundaries_share_one_admission() {
    let io = Pci::new();
    let mut device = Endpoint::discover(io.clone()).unwrap();
    assert_eq!(io.read32(4) & 7, 0);
    assert_eq!(io.read32(0x40) & (1 << 16), 0);
    assert_eq!(device.size(), 1 << 20);
    unsafe { device.place(0x40000000) }.unwrap();
    let owner = Owner::new().unwrap();
    let stranger = Owner::new().unwrap();
    device.claim(&owner).unwrap();
    assert_eq!(device.claim(&stranger), Err(Error::Busy));
    assert_eq!(device.inspect(&owner, 0), Ok(0x11e81234));
    for offset in [4, 0x10, 0x34, 0x40, 0x100, usize::MAX] {
        assert_eq!(device.inspect(&owner, offset), Err(Error::Denied));
    }
    assert_eq!(device.access(&stranger, 0, 4), Err(Error::Denied));
    assert_eq!(device.access(&owner, (1 << 20) - 8, 8), Ok(0x400ffff8));
    for (offset, width) in [(1, 4), (0, 3), ((1 << 20) - 4, 8), (usize::MAX, 1)] {
        assert_eq!(device.access(&owner, offset, width), Err(Error::Invalid));
    }
    unsafe { device.activate(&owner) }.unwrap();
    assert_eq!(io.read32(4) & 7, 6);
    owner.close();
    assert!(device.owner_closed());
    assert_eq!(device.access(&owner, 0, 4), Err(Error::Denied));
    device.retire().unwrap();
    assert_eq!(io.read32(4) & 7, 0);
    assert_eq!(device.phase(), Phase::Retiring); // not a completion proof
    unsafe { device.settle() }.unwrap();
    assert_eq!(device.claim(&stranger), Err(Error::Busy));
}

#[test]
fn unsupported_configuration_never_enables_master_or_leaves_sizing_values() {
    for (offset, value) in [
        (12, 1 << 16),
        (12, 1 << 23),
        (0x100, 0xf),
        (0x40, 0x10),
        (0x40, 0x4005),
        (0x10, 4),
    ] {
        let io = Pci::new();
        io.set(offset, value);
        assert!(matches!(
            Endpoint::discover(io.clone()),
            Err(Error::Unsupported)
        ));
        assert_eq!(io.read32(4) & 7, 0);
        assert!(
            !io.0
                .lock()
                .unwrap()
                .writes
                .iter()
                .any(|&(at, value)| at == 4 && value & 4 != 0)
        );
    }
}

#[test]
fn failed_disable_keeps_endpoint_unusable_and_retryable() {
    let io = Pci::new();
    let mut device = Endpoint::discover(io.clone()).unwrap();
    unsafe { device.place(0x40000000) }.unwrap();
    let owner = Owner::new().unwrap();
    device.claim(&owner).unwrap();
    unsafe { device.activate(&owner) }.unwrap();
    io.0.lock().unwrap().stuck = true;
    assert_eq!(device.retire(), Err(Error::Hardware));
    assert_eq!(device.phase(), Phase::Failed);
    assert_eq!(device.access(&owner, 0, 4), Err(Error::Denied));
    assert!(unsafe { device.settle() }.is_err());
    io.0.lock().unwrap().stuck = false;
    device.retire().unwrap();
    assert_eq!(device.phase(), Phase::Retiring);
}
