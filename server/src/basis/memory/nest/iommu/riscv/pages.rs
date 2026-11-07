use super::EngineError;
use crate::basis::memory::cell::{Cell, Retention};
use alloc::sync::Arc;

/// Queue storage uses ordinary Cells and their existing external obligations.
pub(super) struct Pages {
    pub command: Retention,
    pub fault: Retention,
    pub completion: Retention,
}

impl Pages {
    pub fn new(cells: [Cell; 3]) -> Result<Self, EngineError> {
        let prepare = |cell: Cell| {
            // These uniquely consumed Cells have no users; clear before publication.
            unsafe { cell.as_mut_ptr().write_bytes(0, 4096) };
            Arc::try_new(cell)
                .map_err(|_| EngineError::Memory)?
                .prepare_external(true)
                .map_err(|_| EngineError::Memory)
        };
        let [command, fault, completion] = cells;
        Ok(Self {
            command: prepare(command)?,
            fault: prepare(fault)?,
            completion: prepare(completion)?,
        })
    }

    pub fn publish(&mut self) {
        self.command.publish();
        self.fault.publish();
        self.completion.publish();
    }

    /// # Safety
    /// All submitted commands completed, CQ and FQ are off and not busy, DDTP
    /// stayed Off, PQ stayed disabled, and no late completion write remains.
    pub unsafe fn settle(self) {
        unsafe {
            self.command.retire().settle();
            self.fault.retire().settle();
            self.completion.retire().settle();
        }
    }
}
