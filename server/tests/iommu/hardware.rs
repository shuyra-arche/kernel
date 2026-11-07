//! Only the hardware effect boundary is scripted. Page, IoSpace, state decisions,
//! batch identities, RISC-V encoding and allocator return effects are production code.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use super::iommu::BatchId;
use super::iommu::Hardware;
use super::iommu::HardwareError;
use super::iommu::Projection;

#[derive(Debug, Clone, Copy)]
pub enum Call {
    Publish(Projection),
    Revoke(Projection),
    Fence(Projection, BatchId),
    Observe(BatchId),
}

#[derive(Default)]
pub struct Trace {
    pub calls: Vec<Call>,
    pub completion: VecDeque<Result<Option<BatchId>, HardwareError>>,
    pub fail_publish: Option<HardwareError>,
    pub fail_revoke: Option<HardwareError>,
    pub fail_fence: Option<HardwareError>,
    pub drops: usize,
}

pub struct Scripted(pub Arc<Mutex<Trace>>);

impl Scripted {
    pub fn new() -> (Self, Arc<Mutex<Trace>>) {
        let trace = Arc::new(Mutex::new(Trace::default()));
        (Self(trace.clone()), trace)
    }
}

// SAFETY: this fixture never exposes any address to hardware. All access-ending
// requirements are therefore vacuous. Scripted delayed/error/stale observations
// exercise the production protocol but do not validate a real Hardware adapter.
unsafe impl Hardware for Scripted {
    fn publish(&mut self, projection: Projection) -> Result<(), HardwareError> {
        let mut trace = self.0.lock().unwrap();
        trace.calls.push(Call::Publish(projection));
        trace.fail_publish.take().map_or(Ok(()), Err)
    }
    fn revoke(&mut self, projection: Projection) -> Result<(), HardwareError> {
        let mut trace = self.0.lock().unwrap();
        trace.calls.push(Call::Revoke(projection));
        trace.fail_revoke.take().map_or(Ok(()), Err)
    }
    fn fence(&mut self, projection: Projection, batch: BatchId) -> Result<(), HardwareError> {
        let mut trace = self.0.lock().unwrap();
        trace.calls.push(Call::Fence(projection, batch));
        trace.fail_fence.take().map_or(Ok(()), Err)
    }
    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError> {
        let mut trace = self.0.lock().unwrap();
        trace.calls.push(Call::Observe(batch));
        trace.completion.pop_front().unwrap_or(Ok(None))
    }
}

impl Drop for Scripted {
    fn drop(&mut self) {
        self.0.lock().unwrap().drops += 1;
    }
}
