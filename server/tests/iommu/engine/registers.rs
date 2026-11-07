use crate::iommu::riscv::registers::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Device(pub Arc<Mutex<[u64; 16]>>);

impl Device {
    pub fn new() -> Self {
        let mut words = [0; 16];
        words[CAP / 8] = 0x10 | (56u64 << 32) | (1 << 9) | (1 << 22);
        Self(Arc::new(Mutex::new(words)))
    }

    pub fn set32(&self, offset: usize, value: u32) {
        let mut words = self.0.lock().unwrap();
        let shift = (offset % 8) * 8;
        words[offset / 8] =
            (words[offset / 8] & !(0xffff_ffff << shift)) | ((value as u64) << shift);
    }

    // The fixture emulates only register/queue effects; state and Cells are real.
    pub fn complete(&self, write_completion: bool, advance_head: bool) {
        let tail = self.read32(CQT);
        let base = ((self.read64(CQB) >> 10) << 12) as usize;
        let last = (tail.wrapping_sub(1) & 255) as usize;
        unsafe {
            let slot = (base as *const u64).add(last * 2);
            let low = u64::from_le(slot.read_volatile());
            let high = u64::from_le(slot.add(1).read_volatile());
            assert_eq!(low & 0x7f, 2);
            if write_completion {
                ((high << 2) as *mut u32).write_volatile(((low >> 32) as u32).to_le());
            }
        }
        if advance_head {
            self.set32(CQH, tail);
        }
    }
}

// SAFETY: test-owned storage and serialized access emulate coherent MMIO. The
// fixture makes no claim about real DMA quiescence and is not a Hardware adapter.
unsafe impl Registers for Device {
    fn read32(&self, offset: usize) -> u32 {
        (self.0.lock().unwrap()[offset / 8] >> ((offset % 8) * 8)) as u32
    }
    fn read64(&self, offset: usize) -> u64 {
        self.0.lock().unwrap()[offset / 8]
    }
    fn write32(&mut self, offset: usize, value: u32) {
        if offset == CQCSR || offset == FQCSR {
            self.set32(offset, if value & ENABLE != 0 { ENABLE | ON } else { 0 });
        } else {
            self.set32(offset, value);
        }
    }
    fn write64(&mut self, offset: usize, value: u64) {
        self.0.lock().unwrap()[offset / 8] = value;
    }
    fn sync(&self) {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}
