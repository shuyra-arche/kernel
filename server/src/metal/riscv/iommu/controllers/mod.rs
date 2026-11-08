//! Physical controllers remain in Kernel custody from boot through assignment.
//! Binding an endpoint consumes its one-shot engine; data publication is still
//! performed through the Nest authorization and Memo publication boundary.
use super::mmio::Mmio;
mod pool;

use crate::metal::sync::SpinLock;
pub(crate) use pool::Controllers;

pub(crate) static CONTROLLERS: SpinLock<Option<Controllers<Mmio>>> = SpinLock::new(None);
