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
    Unconfirmed,
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

enum Context<R: Registers> {
    Setup(Engine<R>, [Retention; 4]),
    Domain(Domain<R>),
}

enum Binding<R: Registers> {
    Open(Context<R>),
    Closing {
        context: Context<R>,
        ticket: Retirement,
    },
    Closed(Option<Engine<R>>),
}

pub(crate) struct Adapter<R: Registers, Q: Quiescence> {
    binding: Binding<R>,
    device: u32,
    epoch: BindingId,
    controller: usize,
    proof: Q,
    leaves: Vec<Leaf>,
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
        let epoch = match BindingId::allocate() {
            Ok(id) => id,
            Err(_) => return Err((EngineError::Memory, engine)),
        };
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
            binding: Binding::Open(Context::Setup(engine, tables)),
            device,
            epoch,
            proof,
            leaves,
        })
    }
    fn index(&self, p: Projection) -> Result<usize, HardwareError> {
        self.leaves
            .iter()
            .position(|leaf| leaf.projection == p)
            .ok_or(HardwareError::Fault)
    }
    fn domain(&mut self) -> Result<&mut Domain<R>, HardwareError> {
        match &mut self.binding {
            Binding::Open(Context::Domain(domain))
            | Binding::Closing {
                context: Context::Domain(domain),
                ..
            } => Ok(domain),
            _ => Err(HardwareError::Fault),
        }
    }
    fn publication(&mut self, at: usize) -> Result<bool, HardwareError> {
        let phase = self.leaves[at].publication;
        let domain = self.domain()?;
        let ready = match phase {
            Publication::Starting => domain.step().map_err(failure)? == DomainPhase::Active,
            Publication::Unconfirmed => return Err(HardwareError::Fault),
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
        match &mut self.binding {
            Binding::Closed(engine) => engine.take(),
            _ => None,
        }
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
    fn endpoint(&self) -> Option<crate::metal::generic::memory::Endpoint> {
        Some(crate::metal::generic::memory::Endpoint::External {
            controller: self.controller as u64,
            requester: self.device,
            binding: self.epoch.epoch(),
        })
    }

    fn capacity(&self) -> usize {
        match &self.binding {
            Binding::Open(Context::Setup(..)) => 512,
            Binding::Open(Context::Domain(domain))
                if domain.phase() != DomainPhase::Failed
                    && !self
                        .leaves
                        .iter()
                        .any(|leaf| matches!(leaf.publication, Publication::Unconfirmed)) =>
            {
                512 - self.leaves.len()
            }
            _ => 0,
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
        match &self.binding {
            Binding::Open(Context::Setup(engine, tables)) => {
                Domain::validate(engine, tables, p, self.device).map_err(failure)
            }
            Binding::Open(Context::Domain(domain)) if domain.idle() => {
                domain.validate_leaf(p).map_err(failure)
            }
            _ => Err(HardwareError::Unavailable),
        }
    }
    fn publish(&mut self, p: Projection) -> Result<(), HardwareError> {
        self.validate(p)?;
        let first = matches!(self.binding, Binding::Open(Context::Setup(..)));
        // A later leaf can reach the page table before its own fence submits.
        // It cannot inherit readiness from the already-active context.
        self.leaves.push(Leaf {
            projection: p,
            publication: if first {
                Publication::Starting
            } else {
                Publication::Unconfirmed
            },
            withdrawal: None,
        });
        if first {
            let Binding::Open(Context::Setup(engine, tables)) =
                core::mem::replace(&mut self.binding, Binding::Closed(None))
            else {
                unreachable!("validated setup")
            };
            // SAFETY: validate checked this exact context and projection above;
            // no other writer can replace this exclusive binding.
            let domain = unsafe { Domain::new(engine, tables, p, self.device) };
            self.binding = Binding::Open(Context::Domain(domain));
            self.domain()?.start().map_err(failure)
        } else {
            let batch = self.domain()?.publish_leaf(p).map_err(failure)?;
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
        // Withdrawing a leaf with no submitted publication fence can still
        // succeed: clear its PTE, then prove withdrawal with a fresh fence.
        // This path never certifies that the failed publication was ready.
        if !matches!(self.leaves[at].publication, Publication::Unconfirmed)
            && !self.publication(at)?
        {
            return Ok(None);
        }
        let p = self.leaves[at].projection;
        match self.leaves[at].withdrawal.unwrap().1 {
            Withdrawal::Requested => {
                let domain = self.domain()?;
                if domain.phase() == DomainPhase::Failed {
                    return Err(HardwareError::Fault);
                }
                if !domain.idle() {
                    return Ok(None);
                }
                let fence = domain.withdraw_leaf(p).map_err(failure)?;
                self.leaves[at].withdrawal = Some((batch, Withdrawal::Flushing(fence)));
            }
            Withdrawal::Flushing(fence) => {
                if self.domain()?.observe_leaf(fence).map_err(failure)? {
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
        match self.domain() {
            Ok(domain) => domain.fault().map_err(failure),
            Err(_) => Ok(None),
        }
    }
    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        if matches!(self.binding, Binding::Closed(_)) {
            return Ok(true);
        }
        if !self.leaves.is_empty() {
            return Ok(false);
        }
        if matches!(self.binding, Binding::Open(_)) {
            let ticket = Retirement::new(
                self.controller,
                self.device,
                BindingId::allocate().map_err(|_| HardwareError::Unavailable)?,
                BatchId::allocate().map_err(|_| HardwareError::Unavailable)?,
            );
            let Binding::Open(context) =
                core::mem::replace(&mut self.binding, Binding::Closed(None))
            else {
                unreachable!("open binding")
            };
            self.binding = Binding::Closing { context, ticket };
        }
        let Binding::Closing { context, ticket } = &mut self.binding else {
            unreachable!("closing binding")
        };
        if let Context::Domain(domain) = context {
            if domain.phase() != DomainPhase::Closed {
                match domain.phase() {
                    DomainPhase::Active => domain.revoke().map_err(failure)?,
                    DomainPhase::Blocked => domain.close().map_err(failure)?,
                    _ => {
                        domain.step().map_err(failure)?;
                    }
                }
                return Ok(false);
            }
        }
        // Even an unpublished context needs separate Binding proof before
        // reassignment. Drain faults after proof and before returning the Engine.
        match self.proof.prove(*ticket)? {
            None => return Ok(false),
            Some(proof) if proof == *ticket => {}
            Some(_) => return Err(HardwareError::Fault),
        }
        let fault = match context {
            Context::Setup(engine, _) => engine.fault(),
            Context::Domain(domain) => domain.fault(),
        };
        if fault.map_err(failure)?.is_some() {
            return Ok(false);
        }
        let Binding::Closing { context, .. } =
            core::mem::replace(&mut self.binding, Binding::Closed(None))
        else {
            unreachable!("proved closing binding")
        };
        let engine = match context {
            Context::Setup(engine, _) => engine,
            // SAFETY: the closed Domain and exact platform proof were checked
            // under this exclusive adapter borrow before taking the context.
            Context::Domain(domain) => unsafe { domain.settle() }.map_err(failure)?,
        };
        self.binding = Binding::Closed(Some(engine));
        Ok(true)
    }
}
