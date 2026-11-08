use crate::metal::iommu::BatchId;
mod command;
mod fault;
mod pages;
mod phase;
pub(crate) mod registers;

pub(crate) use command::{Command, fence_complete};

pub(crate) use phase::Phase;

use crate::basis::cell::page::Page;
use pages::Pages;
use phase::Event;
use registers::*;

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

/// One physical command engine, not one engine per Nest. This first adapter
/// keeps DDTP Off until an owning Domain installs a retained directory. Polls
/// with bounded steps; queue shutdown is refused while that directory is active.
/// CQ completion is local evidence; it is deliberately not a Hardware::observe
/// implementation or a proof that an arbitrary device projection is reclaimable.
pub(crate) struct Engine<R: Registers> {
    pub(super) registers: R,
    pages: Option<Pages>,
    phase: Phase,
    tail: u32,
    directory: u64,
    wired: bool,
}

impl<R: Registers> Engine<R> {
    pub(crate) fn physical_base(&self) -> usize {
        self.registers.physical_base()
    }
    pub(crate) fn new(registers: R, cells: [Page; 3]) -> Result<Self, EngineError> {
        Ok(Self {
            registers,
            pages: Some(Pages::new(cells)?),
            phase: Phase::Prepared,
            tail: 0,
            directory: 0,
            wired: false,
        })
    }

    /// Before any queue starts, select wired interrupts for a trusted platform
    /// route. Queue interrupts only prompt observation; never settle a Page.
    pub(crate) fn enable_interrupts(&mut self) -> Result<(), EngineError> {
        if self.phase != Phase::Prepared
            || !self.off()
            || [CQCSR, FQCSR]
                .into_iter()
                .any(|r| self.registers.read32(r) & (ENABLE | ON | BUSY) != 0)
            || !matches!((self.registers.read64(CAP) >> 28) & 3, 1 | 2)
        {
            return Err(EngineError::Unsupported);
        }
        self.registers.write32(FCTL, 2);
        self.registers.write64(ICVEC, 0x3210);
        self.registers.sync();
        if self.registers.read32(FCTL) != 2 || self.registers.read64(ICVEC) != 0x3210 {
            return Err(EngineError::Unsupported);
        }
        self.wired = true;
        Ok(())
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
        if self.registers.read64(DDTP) & !16 != self.directory
            || self.registers.read32(PQCSR) & (ENABLE | ON | BUSY) != 0
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
                    self.registers
                        .write32(CQCSR, ENABLE | if self.wired { INTERRUPT } else { 0 });
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
                    && self.off()
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
        self.registers
            .write32(FQCSR, ENABLE | if self.wired { INTERRUPT } else { 0 });
        Ok(())
    }

    /// Submit a host translation invalidation followed by a uniquely tagged fence.
    /// One batch at a time; a completed slot is reused only after its write AND
    /// command completion were observed. No arbitrary physical write interface.
    pub(crate) fn invalidate(&mut self) -> Result<BatchId, EngineError> {
        self.submit(&[Command::invalidate_host()])
    }

    pub(super) fn invalidate_directory(&mut self) -> Result<BatchId, EngineError> {
        self.submit(&[Command::invalidate_directory(), Command::invalidate_host()])
    }

    fn submit(&mut self, commands: &[Command]) -> Result<BatchId, EngineError> {
        if self.phase != Phase::Ready {
            return Err(EngineError::State);
        }
        self.healthy()?;
        if self.registers.read64(DDTP) & 16 != 0 {
            return Err(EngineError::State);
        }
        if self.registers.read32(CQCSR) & (ENABLE | ON | BUSY) != ENABLE | ON
            || self.registers.read32(CQH) != self.tail
        {
            return self.fail(EngineError::Hardware);
        }
        let batch = BatchId::allocate().map_err(|_| EngineError::Exhausted)?;
        let pages = self.pages.as_ref().expect("live pages");
        let mut fence = Command::fence(batch, pages.completion.physical_address() as u64, true)
            .map_err(|_| EngineError::Address)?;
        if self.wired {
            fence = fence.interrupt();
        }
        // SAFETY: exclusive Page storage, no outstanding batch or completion write.
        unsafe {
            pages
                .completion
                .as_mut_ptr()
                .cast::<u32>()
                .write_volatile(0);
            for (i, command) in commands
                .iter()
                .copied()
                .chain(core::iter::once(fence))
                .enumerate()
            {
                let offset = ((self.tail as usize + i) & 255) * 16;
                let slot = pages.command.as_mut_ptr().add(offset).cast::<u64>();
                let [low, high] = command.words();
                slot.write_volatile(low.to_le());
                slot.add(1).write_volatile(high.to_le());
            }
        }
        self.advance(Event::Submit(batch))?;
        self.tail = (self.tail + commands.len() as u32 + 1) & 255;
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
        if self.wired {
            self.registers
                .write32(CQCSR, ENABLE | INTERRUPT | (1 << 11));
            self.registers.write32(IPSR, 1);
        }
        self.advance(Event::Complete(batch))?;
        Ok(true)
    }

    /// Only the owning Domain may change DDTP; its retained table pages must
    /// already be published before a nonzero directory is passed here.
    pub(super) unsafe fn directory(&mut self, value: u64) -> Result<(), EngineError> {
        if self.phase != Phase::Ready || !self.directory_ready()? {
            return Err(EngineError::State);
        }
        self.directory = value;
        self.registers.sync();
        self.registers.write64(DDTP, value);
        Ok(())
    }

    pub(super) fn directory_ready(&mut self) -> Result<bool, EngineError> {
        // During an asynchronous DDTP update the old mode/PPN may still be read.
        if self.registers.read64(DDTP) & 16 != 0 {
            return Ok(false);
        }
        self.healthy()?;
        Ok(true)
    }

    pub(crate) fn fault(&mut self) -> Result<Option<crate::metal::iommu::Fault>, EngineError> {
        if matches!(self.phase, Phase::Prepared | Phase::Closed | Phase::Failed) {
            return Err(EngineError::State);
        }
        self.healthy()?;
        if self.registers.read32(FQCSR) & (ENABLE | ON | BUSY) != ENABLE | ON {
            return Err(EngineError::State);
        }
        let head = self.registers.read32(FQH);
        let tail = self.registers.read32(FQT);
        if head >= 128 || tail >= 128 {
            return self.fail(EngineError::Hardware);
        }
        if head == tail {
            if self.wired {
                self.registers.write32(IPSR, 2);
            }
            return Ok(None);
        }
        self.registers.sync();
        let page = &self.pages.as_ref().expect("live pages").fault;
        // FQT publishes a complete record. It cannot be overwritten until FQH
        // advances; copy all words before returning its slot to the producer.
        let words = unsafe {
            let ptr = page.as_mut_ptr().add(head as usize * 32).cast::<u64>();
            core::array::from_fn(|i| u64::from_le(ptr.add(i).read_volatile()))
        };
        self.registers.sync();
        self.registers.write32(FQH, (head + 1) & 127);
        Ok(Some(fault::decode(words)))
    }

    pub(crate) fn stop(&mut self) -> Result<(), EngineError> {
        if self.phase == Phase::Prepared {
            self.advance(Event::Stop)?;
            self.pages.take(); // Prepared obligations cancel without hardware effects.
            return Ok(());
        }
        if self.phase != Phase::Ready || self.directory != 0 {
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
