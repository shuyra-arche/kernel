use crate::pci::Config;
use crate::pci::Endpoint;
use crate::pci::Error;
use std::sync::Arc;
use std::sync::Mutex;

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
fn mmio_decode_and_master_enable_are_distinct_hardware_effects() {
    let io = Pci::new();
    let mut port = Endpoint::discover(io.clone()).unwrap();
    assert_eq!(io.read32(4) & 7, 0);
    assert_eq!(io.read32(0x40) & (1 << 16), 0);
    unsafe { port.place(0x40000000) }.unwrap();
    port.decode().unwrap();
    assert_eq!(io.read32(4) & 7, 2);
    unsafe { port.activate() }.unwrap();
    assert_eq!(io.read32(4) & 7, 6);
    port.stop().unwrap();
    assert_eq!(io.read32(4) & 7, 0);
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
fn failed_hardware_admission_close_is_reported_and_retryable() {
    let io = Pci::new();
    let mut port = Endpoint::discover(io.clone()).unwrap();
    unsafe { port.place(0x40000000) }.unwrap();
    port.decode().unwrap();
    unsafe { port.activate() }.unwrap();
    io.0.lock().unwrap().stuck = true;
    assert_eq!(port.stop(), Err(Error::Hardware));
    io.0.lock().unwrap().stuck = false;
    port.stop().unwrap();
    assert_eq!(io.read32(4) & 7, 0);
}
