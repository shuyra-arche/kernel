//! Bounded hardware projection: one controller, one device and one whole Cell.
//! This internal bootstrap mechanism does not assign Unit ownership or implement
//! View/Pend. It is not yet installed by normal boot or exposed through an ABI.
//! Translation withdrawal and platform reclamation proof remain separate steps.
mod phase;

use super::registers::{CAP, FCTL, Registers};
use super::{Engine, EngineError, Fault, Phase};
use crate::core::memory::cell::{Cell, Retention};
use alloc::sync::Arc;
pub(crate) use phase::DomainPhase;
use phase::Event;

pub(crate) struct Domain<R: Registers> {
    engine: Engine<R>,
    // DDT, Sv39 root/middle/leaf, then payload. All are ordinary Cells.
    pages: Option<[Retention; 5]>,
    leaf: usize,
    phase: DomainPhase,
}

impl<R: Registers> Domain<R> {
    /// Consume exclusive Cells; only this one payload page is made device-visible.
    /// The four table pages must be fresh allocations; all are cleared here.
    ///
    /// # Safety
    /// Caller is the trusted Kernel device-binding path: the device ID is correct,
    /// every access from it traverses this IOMMU, and no device uses bypass or
    /// retained ATS translations. CPU/device borrowing and residency have been
    /// authorized for the complete Cell. None of these facts follow from Arc.
    pub(crate) unsafe fn new(
        engine: Engine<R>,
        tables: [Cell; 4],
        payload: Cell,
        device: u32,
        iova: usize,
        modifiable: bool,
    ) -> Result<Self, EngineError> {
        let cap = engine.registers.read64(CAP);
        let pas = (cap >> 32) & 63;
        let stride = if cap & (1 << 22) != 0 { 64 } else { 32 };
        if engine.phase() != Phase::Ready {
            return Err(EngineError::State);
        }
        if cap & (1 << 9) == 0 || engine.registers.read32(FCTL) & 5 != 0 {
            return Err(EngineError::Unsupported);
        }
        if device as usize >= 4096 / stride
            || iova >= (1usize << 38)
            || !iova.is_multiple_of(4096)
            || !(12..=56).contains(&pas)
        {
            return Err(EngineError::Address);
        }
        for page in tables.iter().chain(core::iter::once(&payload)) {
            if (page.physical_address() as u64)
                .checked_add(4095)
                .is_none_or(|last| last >= 1u64 << pas)
            {
                return Err(EngineError::Address);
            }
        }
        let [ddt, root, middle, leaf] = tables;
        let prepare = |cell: Cell| {
            Arc::try_new(cell)
                .map_err(|_| EngineError::Memory)?
                .prepare_external()
                .map_err(|_| EngineError::Memory)
        };
        let pages = [
            prepare(ddt)?,
            prepare(root)?,
            prepare(middle)?,
            prepare(leaf)?,
            prepare(payload)?,
        ];
        for page in &pages[..4] {
            // Uniquely consumed table allocations have no mappings or hardware readers.
            unsafe { page.as_mut_ptr().write_bytes(0, 4096) };
        }
        let result = Self {
            engine,
            pages: Some(pages),
            leaf: (iova >> 12) & 511,
            phase: DomainPhase::Prepared,
        };
        let p = result.pages.as_ref().expect("prepared pages");
        let context = device as usize * stride / 8;
        result.write(
            0,
            context + 3,
            (8u64 << 60) | (p[1].physical_address() as u64 >> 12),
        );
        // Bare G-stage, Sv39 S-stage; ATS/PRI/PDT/MSI remapping disabled.
        result.write(0, context, 1);
        result.write(1, (iova >> 30) & 511, pointer(&p[2]));
        result.write(2, (iova >> 21) & 511, pointer(&p[3]));
        // V/R/U/A, optional W/D. No executable mapping and no hardware A/D writes.
        result.write(
            3,
            result.leaf,
            pointer(&p[4]) | 2 | 16 | 64 | if modifiable { 4 | 128 } else { 0 },
        );
        Ok(result)
    }

    fn write(&self, page: usize, word: usize, value: u64) {
        // Internal indexes are bounded by validated Sv39/device geometry. &mut
        // methods serialize publication/withdrawal, and hardware never writes PTEs.
        unsafe {
            self.pages.as_ref().expect("live pages")[page]
                .as_mut_ptr()
                .cast::<u64>()
                .add(word)
                .write_volatile(value.to_le())
        };
    }

    pub(crate) fn phase(&self) -> DomainPhase {
        self.phase
    }

    fn advance(&mut self, event: Event) -> Result<(), EngineError> {
        self.phase = self.phase.next(event).ok_or(EngineError::State)?;
        Ok(())
    }

    pub(crate) fn start(&mut self) -> Result<(), EngineError> {
        if self.phase != DomainPhase::Prepared {
            return Err(EngineError::State);
        }
        let pages = self.pages.as_mut().expect("live pages");
        for page in pages.iter_mut() {
            page.publish();
        }
        // Flush stale context/PSCID translations while DDTP is still Off. A
        // subsequent directory must not transiently authorize an old cache entry.
        match self.engine.invalidate_directory() {
            Ok(batch) => self.advance(Event::Start(batch))?,
            Err(error) => {
                self.phase = DomainPhase::Failed;
                return Err(error);
            }
        }
        Ok(())
    }

    pub(crate) fn revoke(&mut self) -> Result<(), EngineError> {
        self.advance(Event::Revoke)?;
        self.pages = self.pages.take().map(|p| p.map(Retention::retire));
        self.write(3, self.leaf, 0);
        match self.engine.invalidate_directory() {
            Ok(batch) => self.advance(Event::Submitted(batch)),
            Err(error) => {
                self.phase = DomainPhase::Failed;
                Err(error)
            }
        }
    }

    pub(crate) fn close(&mut self) -> Result<(), EngineError> {
        self.advance(Event::Close)?;
        // Withdrawal has completed, but every table/payload remains retained.
        if let Err(error) = unsafe { self.engine.directory(0) } {
            self.phase = DomainPhase::Failed;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn step(&mut self) -> Result<DomainPhase, EngineError> {
        self.progress()
            .inspect_err(|_| self.phase = DomainPhase::Failed)?;
        Ok(self.phase)
    }

    fn progress(&mut self) -> Result<(), EngineError> {
        match self.phase {
            DomainPhase::Purging(batch) => {
                if self.engine.observe(batch)? {
                    self.advance(Event::Complete(batch))?;
                    let pages = self.pages.as_ref().expect("live pages");
                    let ddtp = ((pages[0].physical_address() as u64 >> 12) << 10) | 2;
                    // All reachable Cells were retained before this publication.
                    unsafe { self.engine.directory(ddtp)? };
                }
            }
            DomainPhase::Installing if self.engine.directory_ready()? => {
                let batch = self.engine.invalidate_directory()?;
                self.advance(Event::Installed(batch))?;
            }
            DomainPhase::Starting(batch) | DomainPhase::Revoking(batch) => {
                if self.engine.observe(batch)? {
                    self.advance(Event::Complete(batch))?;
                }
            }
            DomainPhase::Detaching if self.engine.directory_ready()? => {
                let batch = self.engine.invalidate_directory()?;
                self.advance(Event::Detached(batch))?;
            }
            DomainPhase::Draining(batch) => {
                if self.engine.observe(batch)? {
                    self.advance(Event::Complete(batch))?;
                    self.engine.stop()?;
                }
            }
            DomainPhase::Stopping => {
                if self.engine.step()? == Phase::Closed {
                    self.advance(Event::Stopped)?;
                }
            }
            DomainPhase::Failed => return Err(EngineError::State),
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn fault(&mut self) -> Result<Option<Fault>, EngineError> {
        self.engine
            .fault()
            .inspect_err(|_| self.phase = DomainPhase::Failed)
    }

    /// Closed means translation/queue teardown only. It does NOT itself prove
    /// platform quiescence, and Drop in Closed still retains these five Cells.
    ///
    /// # Safety
    /// Caller independently proved all old device requests and platform-specific
    /// ordering are complete, with no ATS/device cache/bypass path left. DDTP must
    /// remain Off until stale requests can no longer reach reused memory. An FQ
    /// record, driver acknowledgement, or business completion is not this proof.
    pub(crate) unsafe fn settle(mut self) -> Result<(), EngineError> {
        if self.phase != DomainPhase::Closed {
            return Err(EngineError::State);
        }
        for page in self.pages.take().expect("retiring pages") {
            unsafe { page.settle() };
        }
        Ok(())
    }
}

fn pointer(page: &Retention) -> u64 {
    ((page.physical_address() as u64 >> 12) << 10) | 1
}
