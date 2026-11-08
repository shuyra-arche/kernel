//! One-shot, one-device IoSpace backend. IoSpace alone holds the data obligation;
//! Domain holds translation metadata and Engine holds its private queues.
use super::Engine;
use super::EngineError;
use super::Phase;
use super::domain::Domain;
use super::domain::DomainPhase;
use super::registers::Registers;
use crate::basis::cell::page::Retention;
use crate::basis::cell::page::{Page, PageId};
use crate::metal::generic::iommu::Hardware;
use crate::metal::generic::iommu::{Quiescence, Retirement};
use crate::metal::iommu::BatchId;
use crate::metal::iommu::Fault;
use crate::metal::iommu::HardwareError;
use crate::metal::iommu::Projection;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Prepared,
    Published,
    Retiring(Option<BatchId>),
    Settled(Option<BatchId>),
    Failed,
}

pub(crate) struct Adapter<R: Registers, Q: Quiescence> {
    setup: Option<(Engine<R>, [Retention; 4])>,
    domain: Option<Domain<R>>,
    device: u32,
    controller: usize,
    proof: Q,
    projection: Option<Projection>,
    state: State,
}

impl<R: Registers, Q: Quiescence> Adapter<R, Q> {
    /// # Safety
    /// Exclusive trusted controller/device binding: all requests traverse this
    /// IOMMU, no bypass/ATS/PRI, coherent Sv39 tables; the four Cells are fresh.
    /// The proof covers this entire controller, including every possible requester.
    /// This constructor does not authorize CPU/device borrowing (IoSpace::register).
    /// Preparation failure returns the Engine so its live queues can still be stopped.
    pub(super) unsafe fn new(
        engine: Engine<R>,
        tables: [Page; 4],
        device: u32,
        proof: Q,
    ) -> Result<Self, (EngineError, Engine<R>)> {
        if let Err(error) = Domain::supports_device(&engine, device) {
            return Err((error, engine));
        }
        let tables = match Domain::<R>::prepare(tables) {
            Ok(tables) => tables,
            Err(error) => return Err((error, engine)),
        };
        Ok(Self {
            controller: engine.physical_base(),
            setup: Some((engine, tables)),
            domain: None,
            device,
            proof,
            projection: None,
            state: State::Prepared,
        })
    }

    fn matches(&self, p: Projection) -> Result<(), HardwareError> {
        if self.projection == Some(p) {
            Ok(())
        } else {
            Err(HardwareError::Fault)
        }
    }
}

fn failure(error: EngineError) -> HardwareError {
    match error {
        EngineError::Unsupported | EngineError::Address => HardwareError::Unavailable,
        _ => HardwareError::Fault,
    }
}

// SAFETY: construction establishes exclusive platform ownership. Publication is
// restricted to one checked projection, every teardown step is retained, and no
// outer batch completes until Domain is Closed AND independent platform proof
// succeeds. Dropping either live Domain or Engine quarantines its Cells.
unsafe impl<R: Registers, Q: Quiescence> Hardware<PageId> for Adapter<R, Q> {
    fn capacity(&self) -> usize {
        usize::from(!matches!(self.state, State::Settled(_)))
    }

    fn validate(&self, p: Projection) -> Result<(), HardwareError> {
        if self.state != State::Prepared {
            return Err(HardwareError::Unavailable);
        }
        let (engine, tables) = self.setup.as_ref().ok_or(HardwareError::Unavailable)?;
        Domain::validate(engine, tables, p, self.device).map_err(failure)
    }

    fn publish(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.validate(p)?;
        self.projection = Some(p);
        let (engine, tables) = self.setup.take().ok_or(HardwareError::Fault)?;
        // IoSpace publishes the data obligation before entering this method.
        self.state = State::Failed;
        let domain = unsafe { Domain::new(engine, tables, p, self.device) }.map_err(failure)?;
        self.state = State::Published;
        self.domain = Some(domain); // retain even partial start failure
        self.domain.as_mut().unwrap().start().map_err(failure)
    }

    fn ready(&mut self, p: Projection) -> Result<bool, HardwareError> {
        self.matches(p)?;
        if self.state != State::Published {
            return Err(HardwareError::Unavailable);
        }
        Ok(self
            .domain
            .as_mut()
            .ok_or(HardwareError::Fault)?
            .step()
            .map_err(failure)?
            == DomainPhase::Active)
    }

    fn revoke(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.matches(p)?;
        if !matches!(self.state, State::Published | State::Retiring(_)) {
            return Err(HardwareError::Fault);
        }
        self.state = State::Retiring(None);
        Ok(())
    }

    fn fence(&mut self, p: Projection, batch: BatchId) -> Result<(), HardwareError> {
        self.matches(p)?;
        if self.state != State::Retiring(None) {
            return Err(HardwareError::Fault);
        }
        self.state = State::Retiring(Some(batch));
        Ok(())
    }

    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError> {
        if self.state == State::Settled(Some(batch)) {
            return Ok(Some(batch));
        }
        if self.state != State::Retiring(Some(batch)) {
            return Err(HardwareError::Fault);
        }
        let domain = self.domain.as_mut().ok_or(HardwareError::Fault)?;
        match domain.phase() {
            DomainPhase::Active => domain.revoke().map_err(failure)?,
            DomainPhase::Blocked => domain.close().map_err(failure)?,
            DomainPhase::Closed => {
                // Errors leave the Domain and all table/data obligations intact.
                // A retry gets a fresh outer batch and must poll proof again.
                let ticket = Retirement::new(
                    self.controller,
                    self.device,
                    self.projection.ok_or(HardwareError::Fault)?.id,
                    batch,
                );
                match self.proof.poll(ticket)? {
                    None => {}
                    Some(proof) if proof == ticket => {
                        unsafe { self.domain.take().unwrap().settle() }.map_err(failure)?;
                        self.state = State::Settled(Some(batch));
                        return Ok(Some(batch));
                    }
                    Some(_) => return Err(HardwareError::Fault),
                }
            }
            _ => {
                domain.step().map_err(failure)?;
            }
        }
        Ok(None)
    }

    fn fault(&mut self) -> Result<Option<Fault>, HardwareError> {
        self.domain
            .as_mut()
            .ok_or(HardwareError::Unavailable)?
            .fault()
            .map_err(failure)
    }

    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        if matches!(self.state, State::Settled(_)) {
            return Ok(true);
        }
        if self.state != State::Prepared {
            return Err(HardwareError::Fault);
        }
        // No payload ever published: cancel tables, but stop the live command
        // engine asynchronously before returning its three queue Cells.
        let (engine, _) = self.setup.as_mut().ok_or(HardwareError::Fault)?;
        if engine.phase() == Phase::Ready {
            engine.stop().map_err(failure)?;
        }
        if engine.step().map_err(failure)? == Phase::Closed {
            self.setup.take();
            self.state = State::Settled(None);
        }
        Ok(matches!(self.state, State::Settled(_)))
    }
}
