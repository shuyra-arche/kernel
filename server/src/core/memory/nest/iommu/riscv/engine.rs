use super::super::BatchId;
use super::pages::Pages;
use super::phase::Event;
use super::registers::*;
use super::{Command, Phase};
use crate::core::memory::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EngineError {
    State,
    Unsupported,
    ActiveHardware,
    Address,
    Memory,
    Exhausted,
    Hardware,
    WrongBatch,
}

/// One physical command engine, not one engine per IoSpace. This first adapter
/// keeps DDTP Off, never authorizes device DMA, and polls with bounded steps.
/// CQ completion is local evidence; it is deliberately not a Hardware::observe
/// implementation or a proof that an arbitrary device projection is reclaimable.
pub(crate) struct Engine<R: Registers> {
    registers: R,
    pages: Option<Pages>,
    phase: Phase,
    tail: u32,
}

impl<R: Registers> Engine<R> {
    pub(crate) fn new(registers: R, cells: [Cell; 3]) -> Result<Self, EngineError> {
        Ok(Self {
            registers,
            pages: Some(Pages::new(cells)?),
            phase: Phase::Prepared,
            tail: 0,
        })
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    fn advance(&mut self, event: Event) -> Result<(), EngineError> {
        self.phase = self.phase.transition(event).ok_or(EngineError::State)?;
        Ok(())
    }

    fn fail<T>(&mut self, error: EngineError) -> Result<T, EngineError> {
        self.advance(Event::Fail)?;
        Err(error)
    }

    fn off(&self) -> bool {
        self.registers.read64(DDTP) & 0x1f == 0
            && self.registers.read32(PQCSR) & (ENABLE | ON | BUSY) == 0
    }

    fn healthy(&mut self) -> Result<(), EngineError> {
        if !self.off()
            || self.registers.read32(CQCSR) & CQ_ERRORS != 0
            || self.registers.read32(FQCSR) & FQ_ERRORS != 0
        {
            return self.fail(EngineError::Hardware);
        }
        Ok(())
    }

    /// Starts one asynchronous phase or observes its completion. No busy-wait.
    pub(crate) fn step(&mut self) -> Result<Phase, EngineError> {
        match self.phase {
            Phase::Prepared => self.start()?,
            Phase::StartingFaults => {
                self.healthy()?;
                if self.registers.read32(FQCSR) & (ENABLE | ON | BUSY) == ENABLE | ON {
                    if self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) != 0 {
                        return self.fail(EngineError::ActiveHardware);
                    }
                    self.advance(Event::FaultsOn)?;
                    self.registers.write32(CQCSR, ENABLE);
                }
            }
            Phase::StartingCommands => {
                self.healthy()?;
                if self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) == ENABLE | ON {
                    self.advance(Event::CommandsOn)?;
                }
            }
            Phase::StoppingCommands => {
                self.healthy()?;
                if self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) == 0
                    && self.registers.read32(FQCSR) & BUSY == 0
                {
                    self.advance(Event::CommandsOff)?;
                    self.registers.write32(FQCSR, 0);
                }
            }
            Phase::StoppingFaults => {
                self.healthy()?;
                if self.registers.read32(FQCSR) & (ENABLE | ON | BUSY) == 0
                    && self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) == 0
                {
                    self.registers.sync();
                    self.advance(Event::FaultsOff)?;
                    // SAFETY: stop only admits Ready (every submitted fence was
                    // observed with its CQ head past it); CQ/FQ are now off, PQ
                    // stayed disabled and DDTP stayed Off. No writers remain.
                    unsafe { self.pages.take().expect("live pages").settle() };
                }
            }
            Phase::Ready | Phase::Closed => {}
            Phase::Waiting(_) | Phase::Failed => return Err(EngineError::State),
        }
        Ok(self.phase)
    }

    fn start(&mut self) -> Result<(), EngineError> {
        let capability = self.registers.read64(CAP);
        let pas = (capability >> 32) & 0x3f;
        if capability & 0xff != 0x10
            || !(12..=56).contains(&pas)
            || self.registers.read32(FCTL) & 1 != 0
        {
            return Err(EngineError::Unsupported);
        }
        if !self.off()
            || [CQCSR, FQCSR]
                .into_iter()
                .any(|r| self.registers.read32(r) & (ENABLE | ON | BUSY) != 0)
        {
            return Err(EngineError::ActiveHardware);
        }
        let pages = self.pages.as_mut().expect("prepared pages");
        for page in [&pages.command, &pages.fault, &pages.completion] {
            if (page.physical_address() as u64) + 4095 >= 1u64 << pas {
                return Err(EngineError::Address);
            }
        }
        let cq = ((pages.command.physical_address() as u64 >> 12) << 10) | 7;
        let fq = ((pages.fault.physical_address() as u64 >> 12) << 10) | 6;
        // Publish every obligation BEFORE exposing even a queue base register.
        pages.publish();
        self.advance(Event::Start)?;
        self.registers.sync();
        self.registers.write64(CQB, cq);
        self.registers.write64(FQB, fq);
        if self.registers.read64(CQB) != cq || self.registers.read64(FQB) != fq {
            return self.fail(EngineError::Unsupported);
        }
        self.registers.write32(CQT, 0);
        self.registers.write32(FQH, 0);
        self.registers.write32(FQCSR, ENABLE);
        Ok(())
    }

    /// Submit a host translation invalidation followed by a uniquely tagged fence.
    /// One batch at a time; a completed slot is reused only after its write AND
    /// command completion were observed. No arbitrary physical write interface.
    pub(crate) fn invalidate(&mut self) -> Result<BatchId, EngineError> {
        if self.phase != Phase::Ready {
            return Err(EngineError::State);
        }
        self.healthy()?;
        if self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) != ENABLE | ON
            || self.registers.read32(CQH) != self.tail
        {
            return self.fail(EngineError::Hardware);
        }
        let batch = BatchId::allocate().map_err(|_| EngineError::Exhausted)?;
        let pages = self.pages.as_ref().expect("live pages");
        let fence = Command::fence(batch, pages.completion.physical_address() as u64, true)
            .map_err(|_| EngineError::Address)?;
        // SAFETY: exclusive Cell storage, no outstanding batch or completion write.
        unsafe {
            pages
                .completion
                .as_mut_ptr()
                .cast::<u32>()
                .write_volatile(0);
            for (i, command) in [Command::invalidate_host(), fence].into_iter().enumerate() {
                let offset = ((self.tail as usize + i) & 255) * 16;
                let slot = pages.command.as_mut_ptr().add(offset).cast::<u64>();
                let [low, high] = command.words();
                slot.write_volatile(low.to_le());
                slot.add(1).write_volatile(high.to_le());
            }
        }
        self.advance(Event::Submit(batch))?;
        self.tail = (self.tail + 2) & 255;
        self.registers.sync();
        self.registers.write32(CQT, self.tail);
        Ok(batch)
    }

    pub(crate) fn observe(&mut self, batch: BatchId) -> Result<bool, EngineError> {
        let Phase::Waiting(expected) = self.phase else {
            return Err(EngineError::State);
        };
        if batch != expected {
            return Err(EngineError::WrongBatch);
        }
        self.healthy()?;
        let status = self.registers.read32(CQCSR);
        self.registers.sync();
        // SAFETY: live coherent hardware-owned completion word, read via volatile.
        let completion = unsafe {
            self.pages
                .as_ref()
                .expect("live pages")
                .completion
                .as_mut_ptr()
                .cast::<u32>()
                .read_volatile()
        };
        if !super::fence_complete(batch, status, u32::from_le(completion))
            .map_err(|_| EngineError::Hardware)?
            || self.registers.read32(CQH) != self.tail
        {
            return Ok(false);
        }
        self.registers.sync();
        self.advance(Event::Complete(batch))?;
        Ok(true)
    }

    pub(crate) fn stop(&mut self) -> Result<(), EngineError> {
        if self.phase == Phase::Prepared {
            self.advance(Event::Stop)?;
            self.pages.take(); // Prepared obligations cancel without hardware effects.
            return Ok(());
        }
        if self.phase != Phase::Ready {
            return Err(EngineError::State);
        }
        self.healthy()?;
        if self.registers.read32(CQCSR) & BUSY != 0 {
            return Err(EngineError::State);
        }
        self.advance(Event::Stop)?;
        self.registers.write32(CQCSR, 0);
        Ok(())
    }
}

// Default Drop intentionally drops Retentions. Published/failed engines retain
// all three Cells permanently; only the explicit proven-off path settles them.
