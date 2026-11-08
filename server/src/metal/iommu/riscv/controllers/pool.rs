//! Controller ownership is consumed by a validated route, never selected by a
//! caller-provided device ID alone. One-shot assignment matches the current
//! whole-controller Domain; sharing/rebinding requires a separate protocol.
use super::super::Engine;
use super::super::EngineError;
use super::super::adapter::Adapter;
use super::super::registers::Registers;
use super::super::topology::RouteError;
use super::super::topology::Topology;
use crate::basis::cell::page::Page;
use crate::metal::generic::iommu::Quiescence;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindError {
    Route(RouteError),
    UnknownController,
    Unavailable,
    Engine(EngineError),
    Memory,
}

enum Slot<R: Registers> {
    Vacant,
    Available(Engine<R>),
    Assigned,
}

pub(crate) struct Controllers<R: Registers> {
    topology: Topology,
    slots: Vec<(usize, Slot<R>)>,
}

impl<R: Registers> Controllers<R> {
    pub(crate) fn new(topology: Topology) -> Result<Self, BindError> {
        let mut slots = Vec::new();
        for base in topology.controllers() {
            slots.try_reserve(1).map_err(|_| BindError::Memory)?;
            slots.push((base, Slot::Vacant));
        }
        Ok(Self { topology, slots })
    }

    /// Registration precedes starting queues. Refusal returns the engine so an
    /// already started caller can still stop it rather than losing responsibility.
    pub(crate) fn install(&mut self, engine: Engine<R>) -> Result<(), (BindError, Engine<R>)> {
        let Some((_, slot)) = self
            .slots
            .iter_mut()
            .find(|(base, _)| *base == engine.physical_base())
        else {
            return Err((BindError::UnknownController, engine));
        };
        if !matches!(slot, Slot::Vacant) {
            return Err((BindError::Unavailable, engine));
        }
        *slot = Slot::Available(engine);
        Ok(())
    }

    pub(crate) fn engine(&mut self, base: usize) -> Result<&mut Engine<R>, BindError> {
        match self.slots.iter_mut().find(|(b, _)| *b == base) {
            Some((_, Slot::Available(engine))) => Ok(engine),
            Some(_) => Err(BindError::Unavailable),
            None => Err(BindError::UnknownController),
        }
    }

    /// # Safety
    /// Firmware describes the actual immutable routing; caller controls device
    /// ownership and excludes bypass/ATS/PRI. Proof covers this controller's
    /// requesters. This is not a Pend/Page authority: IoSpace::register still requires
    /// separate borrowing/residency authorization. Tables are fresh allocations.
    pub(crate) unsafe fn bind<Q: Quiescence>(
        &mut self,
        host: usize,
        requester: u16,
        tables: [Page; 4],
        proof: Q,
    ) -> Result<Adapter<R, Q>, BindError> {
        let route = self
            .topology
            .resolve(host, requester)
            .map_err(BindError::Route)?;
        let (_, slot) = self
            .slots
            .iter_mut()
            .find(|(base, _)| *base == route.controller)
            .ok_or(BindError::UnknownController)?;
        if !matches!(slot, Slot::Available(_)) {
            return Err(BindError::Unavailable);
        }
        let Slot::Available(engine) = core::mem::replace(slot, Slot::Assigned) else {
            unreachable!()
        };
        // The controller identity comes from Registers' trusted physical mapping,
        // while DID comes from the validated firmware map. Neither comes from IOVA.
        match unsafe { Adapter::new(engine, tables, route.device, proof) } {
            Ok(adapter) => Ok(adapter),
            Err((error, engine)) => {
                *slot = Slot::Available(engine);
                Err(BindError::Engine(error))
            }
        }
    }

    pub(crate) fn unassigned(&mut self) -> impl Iterator<Item = &mut Engine<R>> {
        self.slots.iter_mut().filter_map(|(_, slot)| match slot {
            Slot::Available(engine) => Some(engine),
            _ => None,
        })
    }
}
