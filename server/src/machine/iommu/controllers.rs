//! Physical controllers remain in Kernel custody from boot through assignment.
//! Binding an endpoint consumes its one-shot engine; data publication is still
//! performed through the retirement Registry and IoSpace authorization boundary.
use super::Mmio;
use crate::core::memory::nest::iommu::riscv::controllers::Controllers;
use crate::machine::sync::SpinLock;

pub(crate) static CONTROLLERS: SpinLock<Option<Controllers<Mmio>>> = SpinLock::new(None);
