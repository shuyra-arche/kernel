//! Bounded per-leaf publication and retirement within one requester context.
//! Nest retains payloads; Domain retains tables; Engine retains its own queues.
use super::domain::{Domain, DomainPhase};
use super::registers::Registers;
use super::{Engine, EngineError};
use crate::basis::cell::page::{Page, PageId, Retention};
use crate::metal::generic::iommu::{BindingId, Hardware, Quiescence, Retirement};
use crate::metal::iommu::{BatchId, Fault, HardwareError, Projection};
use alloc::vec::Vec;

#[derive(Clone, Copy)]
enum Publication {
    Starting,
    Flushing(BatchId),
    Ready,
}
#[derive(Clone, Copy)]
enum Withdrawal {
    Requested,
    Flushing(BatchId),
    Proof,
}
struct Leaf {
    projection: Projection,
    publication: Publication,
    withdrawal: Option<(BatchId, Withdrawal)>,
}

pub(crate) struct Adapter<R: Registers, Q: Quiescence> {
    setup: Option<(Engine<R>, [Retention; 4])>,
    domain: Option<Domain<R>>,
    returned: Option<Engine<R>>,
    device: u32,
    controller: usize,
    proof: Q,
    leaves: Vec<Leaf>,
    closing: Option<Retirement>,
    closed: bool,
}

impl<R: Registers, Q: Quiescence> Adapter<R, Q> {
    /// # Safety
    /// Exclusive immutable route, coherent tables, no bypass/ATS/PRI. The proof
    /// covers range withdrawal and ownership replacement of this isolation group.
    pub(super) unsafe fn new(
        engine: Engine<R>,
        tables: [Page; 4],
        device: u32,
        proof: Q,
    ) -> Result<Self, (EngineError, Engine<R>)> {
        if let Err(error) = Domain::supports_device(&engine, device) {
            return Err((error, engine));
        }
        let mut leaves = Vec::new();
        if leaves.try_reserve_exact(512).is_err() {
            return Err((EngineError::Memory, engine));
        }
        let tables = match Domain::<R>::prepare(tables) {
            Ok(tables) => tables,
            Err(error) => return Err((error, engine)),
        };
        Ok(Self {
            controller: engine.physical_base(),
            setup: Some((engine, tables)),
            domain: None,
            returned: None,
            device,
            proof,
            leaves,
            closing: None,
            closed: false,
        })
    }
    fn index(&self, p: Projection) -> Result<usize, HardwareError> {
        self.leaves
            .iter()
            .position(|leaf| leaf.projection == p)
            .ok_or(HardwareError::Fault)
    }
    fn publication(&mut self, at: usize) -> Result<bool, HardwareError> {
        let domain = self.domain.as_mut().ok_or(HardwareError::Fault)?;
        let ready = match self.leaves[at].publication {
            Publication::Starting => domain.step().map_err(failure)? == DomainPhase::Active,
            Publication::Flushing(batch) => domain.observe_leaf(batch).map_err(failure)?,
            Publication::Ready => true,
        };
        if ready {
            self.leaves[at].publication = Publication::Ready;
        }
        Ok(ready)
    }
    /// Only shutdown, after the exact Binding proof, makes this engine available
    /// for reuse. Dropping an unclosed adapter quarantines its live obligations.
    pub(crate) fn take_engine(&mut self) -> Option<Engine<R>> {
        self.returned.take()
    }
}
fn failure(error: EngineError) -> HardwareError {
    match error {
        EngineError::Unsupported | EngineError::Address => HardwareError::Unavailable,
        _ => HardwareError::Fault,
    }
}

// SAFETY: each unique projection retains its own phase/batch; only an invalid
// leaf + exact PR/PW fence + matching platform proof discharges that payload.
// Closing a context requires every leaf settled and a separate Binding proof.
unsafe impl<R: Registers, Q: Quiescence> Hardware<PageId> for Adapter<R, Q> {
    fn capacity(&self) -> usize {
        if self.closing.is_some() || self.closed {
            0
        } else {
            512 - self.leaves.len()
        }
    }
    fn validate(&self, p: Projection) -> Result<(), HardwareError> {
        if self.capacity() == 0
            || self.leaves.iter().any(|leaf| {
                leaf.projection.iova.get() < p.iova.get().saturating_add(p.size)
                    && p.iova.get()
                        < leaf
                            .projection
                            .iova
                            .get()
                            .saturating_add(leaf.projection.size)
            })
        {
            return Err(HardwareError::Unavailable);
        }
        if let Some((engine, tables)) = &self.setup {
            Domain::validate(engine, tables, p, self.device).map_err(failure)
        } else {
            let domain = self.domain.as_ref().ok_or(HardwareError::Unavailable)?;
            if !domain.idle() {
                return Err(HardwareError::Unavailable);
            }
            domain.validate_leaf(p).map_err(failure)
        }
    }
    fn publish(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.validate(p)?;
        // Reserve before the first write; partial failure never loses this identity.
        self.leaves.push(Leaf {
            projection: p,
            publication: Publication::Starting,
            withdrawal: None,
        });
        if let Some((engine, tables)) = self.setup.take() {
            let domain = unsafe { Domain::new(engine, tables, p, self.device) }.map_err(failure)?;
            self.domain = Some(domain);
            self.domain.as_mut().unwrap().start().map_err(failure)
        } else {
            let batch = self
                .domain
                .as_mut()
                .ok_or(HardwareError::Fault)?
                .publish_leaf(p)
                .map_err(failure)?;
            self.leaves.last_mut().unwrap().publication = Publication::Flushing(batch);
            Ok(())
        }
    }
    fn ready(&mut self, p: Projection) -> Result<bool, HardwareError> {
        let at = self.index(p)?;
        if self.leaves[at].withdrawal.is_some() {
            return Err(HardwareError::Unavailable);
        }
        self.publication(at)
    }
    fn revoke(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.index(p)?;
        Ok(()) // fence registers the exact fresh attempt; observe advances it.
    }
    fn fence(&mut self, p: Projection, batch: BatchId) -> Result<(), HardwareError> {
        let at = self.index(p)?;
        // Retrying a platform error keeps the completed hardware fence but must
        // obtain fresh evidence for the new token; never resubmit over a live CQ.
        let phase = self.leaves[at]
            .withdrawal
            .map_or(Withdrawal::Requested, |(_, phase)| phase);
        self.leaves[at].withdrawal = Some((batch, phase));
        Ok(())
    }
    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError> {
        let at = self
            .leaves
            .iter()
            .position(|leaf| leaf.withdrawal.is_some_and(|(b, _)| b == batch))
            .ok_or(HardwareError::Fault)?;
        if !self.publication(at)? {
            return Ok(None);
        }
        let p = self.leaves[at].projection;
        match self.leaves[at].withdrawal.unwrap().1 {
            Withdrawal::Requested => {
                let domain = self.domain.as_mut().ok_or(HardwareError::Fault)?;
                if !domain.idle() {
                    return Ok(None);
                }
                let fence = domain.withdraw_leaf(p).map_err(failure)?;
                self.leaves[at].withdrawal = Some((batch, Withdrawal::Flushing(fence)));
            }
            Withdrawal::Flushing(fence) => {
                if self
                    .domain
                    .as_mut()
                    .ok_or(HardwareError::Fault)?
                    .observe_leaf(fence)
                    .map_err(failure)?
                {
                    self.leaves[at].withdrawal = Some((batch, Withdrawal::Proof));
                }
            }
            Withdrawal::Proof => {
                let ticket = Retirement::new(self.controller, self.device, p.id, batch)
                    .range(p.iova.get(), p.size);
                match self.proof.prove(ticket)? {
                    None => {}
                    Some(proof) if proof == ticket => {
                        self.leaves.remove(at);
                        return Ok(Some(batch));
                    }
                    Some(_) => return Err(HardwareError::Fault),
                }
            }
        }
        Ok(None)
    }
    fn fault(&mut self) -> Result<Option<Fault>, HardwareError> {
        match self.domain.as_mut() {
            Some(domain) => domain.fault().map_err(failure),
            None => Ok(None),
        }
    }
    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        if self.closed {
            return Ok(true);
        }
        if !self.leaves.is_empty() {
            return Ok(false);
        }
        if self.closing.is_none() {
            self.closing = Some(Retirement::new(
                self.controller,
                self.device,
                BindingId::allocate().map_err(|_| HardwareError::Unavailable)?,
                BatchId::allocate().map_err(|_| HardwareError::Unavailable)?,
            ));
        }
        if let Some((engine, _)) = self.setup.as_mut() {
            // Even an unpublished binding may have delayed requests queued in
            // its device. Reassignment still needs the independent Binding proof.
            let ticket = self.closing.unwrap();
            match self.proof.prove(ticket)? {
                None => return Ok(false),
                Some(proof) if proof == ticket => {}
                Some(_) => return Err(HardwareError::Fault),
            }
            if engine.fault().map_err(failure)?.is_some() {
                return Ok(false);
            }
            self.returned = Some(self.setup.take().unwrap().0);
            self.closed = true;
            return Ok(true);
        }
        let domain = self.domain.as_mut().ok_or(HardwareError::Fault)?;
        match domain.phase() {
            DomainPhase::Active => domain.revoke().map_err(failure)?,
            DomainPhase::Blocked => domain.close().map_err(failure)?,
            DomainPhase::Closed => {
                let ticket = self.closing.unwrap();
                match self.proof.prove(ticket)? {
                    None => {}
                    Some(proof) if proof == ticket => {
                        // Drain diagnostic records before the engine is reassigned.
                        // Bounded polls retain the context if more evidence remains.
                        if domain.fault().map_err(failure)?.is_some() {
                            return Ok(false);
                        }
                        self.returned =
                            Some(unsafe { self.domain.take().unwrap().settle() }.map_err(failure)?);
                        self.closed = true;
                        return Ok(true);
                    }
                    Some(_) => return Err(HardwareError::Fault),
                }
            }
            _ => {
                domain.step().map_err(failure)?;
            }
        }
        Ok(false)
    }
}
