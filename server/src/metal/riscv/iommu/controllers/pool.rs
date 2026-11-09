//! Controller ownership is leased only by a validated route. A settled context
//! returns its engine; failed or abandoned bindings remain assigned.
use super::super::Engine;
use super::super::EngineError;
use super::super::adapter::Adapter;
use super::super::registers::Registers;
use crate::basis::cell::page::Page;
use crate::metal::generic::iommu::Quiescence;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindError {
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
    slots: Vec<(usize, Slot<R>)>,
}

impl<R: Registers> Controllers<R> {
    pub(crate) fn new(bases: impl IntoIterator<Item = usize>) -> Result<Self, BindError> {
        let mut slots = Vec::new();
        for base in bases {
            if base == 0 || !base.is_multiple_of(4096) || slots.iter().any(|(old, _)| *old == base)
            {
                return Err(BindError::UnknownController);
            }
            slots.try_reserve(1).map_err(|_| BindError::Memory)?;
            slots.push((base, Slot::Vacant));
        }
        Ok(Self { slots })
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
    /// requesters. This is not a Pend/Page authority: Nest publication admission still requires
    /// separate borrowing/residency authorization. Tables are fresh allocations.
    pub(crate) unsafe fn bind<Q: Quiescence>(
        &mut self,
        controller: usize,
        requester: u32,
        tables: [Page; 4],
        proof: Q,
    ) -> Result<Adapter<R, Q>, BindError> {
        if requester >= 64 {
            return Err(BindError::UnknownController);
        }
        let (_, slot) = self
            .slots
            .iter_mut()
            .find(|(base, _)| *base == controller)
            .ok_or(BindError::UnknownController)?;
        if !matches!(slot, Slot::Available(_)) {
            return Err(BindError::Unavailable);
        }
        let Slot::Available(engine) = core::mem::replace(slot, Slot::Assigned) else {
            unreachable!()
        };
        // The controller identity comes from Registers' trusted physical mapping,
        // while DID comes from the validated firmware map. Neither comes from IOVA.
        match unsafe { Adapter::new(engine, tables, requester, proof) } {
            Ok(adapter) => Ok(adapter),
            Err((error, engine)) => {
                *slot = Slot::Available(engine);
                Err(BindError::Engine(error))
            }
        }
    }

    /// Return only an engine whose context and platform ownership were settled.
    pub(crate) fn restore(&mut self, engine: Engine<R>) -> Result<(), BindError> {
        let (_, slot) = self
            .slots
            .iter_mut()
            .find(|(base, _)| *base == engine.physical_base())
            .ok_or(BindError::UnknownController)?;
        if !matches!(slot, Slot::Assigned) || engine.phase() != super::super::Phase::Ready {
            return Err(BindError::Unavailable);
        }
        *slot = Slot::Available(engine);
        Ok(())
    }

    pub(crate) fn unassigned(&mut self) -> impl Iterator<Item = &mut Engine<R>> {
        self.slots.iter_mut().filter_map(|(_, slot)| match slot {
            Slot::Available(engine) => Some(engine),
            _ => None,
        })
    }
}
