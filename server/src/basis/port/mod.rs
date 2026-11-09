//! A trusted external access source and its real isolation boundary. Firmware
//! establishes identity; Pend grants operations. No Page or Memo lives here.
use crate::basis::{cell::page::PageId, nest::PublicationError};
use crate::metal::{generic::iommu::Port as HardwarePort, iommu::Backend, sync::SpinLock};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use kernel_api::{abi::call::pend::PendFail, model::port::PortId};

pub(crate) struct Port {
    id: PortId,
    pub(crate) controller: usize,
    pub(crate) requester: u32,
    hardware: SpinLock<Box<dyn HardwarePort<PageId, Error = PublicationError>>>,
}
pub(crate) static PORTS: SpinLock<Vec<Arc<Port>>> = SpinLock::new(Vec::new());
impl Port {
    pub(crate) fn register(
        controller: usize,
        requester: u32,
        hardware: Box<dyn HardwarePort<PageId, Error = PublicationError>>,
    ) -> Result<(), PendFail> {
        static NEXT: crate::basis::name::Serial =
            crate::basis::name::Serial::new(1, isize::MAX as usize);
        let mut ports = PORTS.lock();
        // The initial backend owns the entire controller. Unknown aliases or
        // multiple requesters cannot be treated as independently isolated.
        if ports.iter().any(|port| port.controller == controller) {
            return Err(PendFail::Denied);
        }
        ports.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        let id = NEXT
            .allocate()
            .and_then(PortId::new)
            .ok_or(PendFail::OutOfMemory)?;
        ports.push(
            Arc::try_new(Self {
                id,
                controller,
                requester,
                hardware: SpinLock::new(hardware),
            })
            .map_err(|_| PendFail::OutOfMemory)?,
        );
        Ok(())
    }
    pub(crate) fn id(&self) -> PortId {
        self.id
    }
    pub(crate) fn bind(&self) -> Result<Backend, PublicationError> {
        self.hardware.lock().bind()
    }
}
