//! Task-owned optional architectural state. No register image belongs to a Hart.
mod float;
pub(crate) mod instruction;

use alloc::boxed::Box;
use float::Float;

/// The current RV64GC admission contract provides FLEN=64. Q is deliberately
/// not enabled: a 64-bit image cannot preserve a wider physical register file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Capability {
    pub float: bool,
}

#[derive(Default)]
pub(crate) struct ExtendedContext {
    float: Option<Box<Float>>,
}

impl ExtendedContext {
    pub(crate) const fn new() -> Self {
        Self { float: None }
    }

    pub(crate) fn compatible(&self, target: Capability) -> bool {
        self.float.is_none() || target.float
    }

    /// Commit only after allocation succeeds; failure leaves an unused context.
    pub(crate) fn enable(
        &mut self,
        instruction: u32,
        status: &mut usize,
        caps: Capability,
    ) -> Result<bool, ()> {
        if *status & FLOAT_MASK != 0 || !caps.float || !instruction::float(instruction) {
            return Ok(false);
        }
        if self.float.is_some() {
            return Ok(false);
        }
        let image = Box::try_new(Float::zero()).map_err(|_| ())?;
        self.float = Some(image);
        *status = (*status & !FLOAT_MASK) | FLOAT_CLEAN;
        Ok(true)
    }

    #[cfg(target_arch = "riscv64")]
    pub(crate) unsafe fn capture(&mut self, status: usize) {
        if status & FLOAT_MASK != 0 {
            let image = self.float.as_mut().expect("enabled FS without Task image");
            unsafe { image.capture() };
        }
    }

    #[cfg(target_arch = "riscv64")]
    pub(crate) unsafe fn restore(&self, status: &mut usize) {
        *status &= !FLOAT_MASK;
        if let Some(image) = &self.float {
            unsafe { image.restore() };
            *status |= FLOAT_CLEAN;
        }
    }
}

const FLOAT_MASK: usize = 3 << 13;
const FLOAT_CLEAN: usize = 2 << 13;
