//! Task-owned optional architectural state. No register image belongs to a Hart.
mod float;
#[cfg(target_arch = "riscv64")]
pub(crate) mod hardware;
mod vector;
pub(crate) use vector::Shape;
pub(crate) mod instruction;

use alloc::boxed::Box;
use float::Float;

/// The current RV64GC admission contract provides FLEN=64. Q is deliberately
/// not enabled: a 64-bit image cannot preserve a wider physical register file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Capability {
    pub float: bool,
    pub vector: Option<Shape>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreparationError {
    Unsupported,
    OutOfMemory,
}

#[derive(Default)]
pub(crate) struct ExtendedContext {
    float: Option<Box<Float>>,
    vector: Option<vector::Vector>,
}

impl ExtendedContext {
    pub(crate) const fn new() -> Self {
        Self {
            float: None,
            vector: None,
        }
    }

    pub(crate) fn compatible(&self, target: Capability) -> bool {
        (self.float.is_none() || target.float)
            && self
                .vector
                .as_ref()
                .is_none_or(|image| Some(image.shape()) == target.vector)
    }

    /// Prepare every declared image while the Task is stopped. No first-use
    /// allocation is permitted after real-time admission.
    pub(crate) fn prepare(&mut self, caps: Capability, flags: u64) -> Result<(), PreparationError> {
        if flags & !3 != 0
            || flags != 0 && !caps.float
            || flags & 2 != 0 && caps.vector.is_none()
            || self.float.is_some() && flags == 0
            || self.vector.is_some() && flags & 2 == 0
            || !self.compatible(caps)
        {
            return Err(PreparationError::Unsupported);
        }
        let float = if flags != 0 && self.float.is_none() {
            Some(Box::try_new(Float::zero()).map_err(|_| PreparationError::OutOfMemory)?)
        } else {
            None
        };
        let vector = if flags & 2 != 0 && self.vector.is_none() {
            Some(
                vector::Vector::new(caps.vector.ok_or(PreparationError::Unsupported)?)
                    .map_err(|_| PreparationError::OutOfMemory)?,
            )
        } else {
            None
        };
        if let Some(image) = float {
            self.float = Some(image);
        }
        if let Some(image) = vector {
            self.vector = Some(image);
        }
        Ok(())
    }

    /// Commit only after allocation succeeds; failure leaves an unused context.
    pub(crate) fn enable(
        &mut self,
        instruction: u32,
        status: &mut usize,
        caps: Capability,
    ) -> Result<bool, ()> {
        let vector_use = instruction::vector(instruction);
        let float_use = instruction::float(instruction) || instruction::vector_float(instruction);
        let need_vector = vector_use && *status & VECTOR_MASK == 0;
        let need_float = float_use && *status & FLOAT_MASK == 0;
        if (!need_vector && !need_float)
            || (vector_use && caps.vector.is_none())
            || (need_float && !caps.float)
            || (need_float && self.float.is_some())
            || (need_vector && self.vector.is_some())
        {
            return Ok(false);
        }
        // Both allocations precede publication. Failure cannot leave half an
        // enabled vector-floating instruction or modify the saved status.
        let float = if need_float {
            Some(Box::try_new(Float::zero()).map_err(|_| ())?)
        } else {
            None
        };
        let vector = if need_vector {
            Some(vector::Vector::new(caps.vector.unwrap())?)
        } else {
            None
        };
        if let Some(image) = float {
            self.float = Some(image);
            *status |= FLOAT_CLEAN;
        }
        if let Some(image) = vector {
            self.vector = Some(image);
            *status |= VECTOR_CLEAN;
        }
        Ok(true)
    }

    #[cfg(target_arch = "riscv64")]
    pub(crate) unsafe fn capture(&mut self, status: usize) {
        if status & VECTOR_MASK != 0 {
            let image = self.vector.as_mut().expect("enabled VS without Task image");
            unsafe { image.capture() };
        }
        if status & FLOAT_MASK != 0 {
            let image = self.float.as_mut().expect("enabled FS without Task image");
            unsafe { image.capture() };
        }
    }

    #[cfg(target_arch = "riscv64")]
    pub(crate) unsafe fn restore(&self, status: &mut usize) {
        *status &= !(FLOAT_MASK | VECTOR_MASK);
        if let Some(image) = &self.vector {
            unsafe { image.restore() };
            *status |= VECTOR_CLEAN;
        }
        if let Some(image) = &self.float {
            unsafe { image.restore() };
            *status |= FLOAT_CLEAN;
        }
    }
}

const FLOAT_MASK: usize = 3 << 13;
const FLOAT_CLEAN: usize = 2 << 13;

const VECTOR_MASK: usize = 3 << 9;
const VECTOR_CLEAN: usize = 2 << 9;
