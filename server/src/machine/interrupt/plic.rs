use super::{Claim, Controller, Irq};

/// Access to one S-mode context of an exclusively owned PLIC. The route must
/// serialize MMIO on its Hart with interrupts masked.
pub(crate) struct Plic {
    base: usize,
    physical: usize,
    context: usize,
}

impl Plic {
    /// # Safety
    /// The mapping covers the validated controller and all passed S-mode
    /// contexts. Kernel owns these contexts and the supplied interrupt lines.
    pub(crate) unsafe fn new(base: usize, physical: usize, context: usize) -> Self {
        Self {
            base,
            physical,
            context,
        }
    }

    /// # Safety
    /// These contexts and lines were validated within the owned mapping.
    pub(crate) unsafe fn configure(&self, contexts: &[usize], sources: usize, lines: [u32; 3]) {
        let base = self.base;
        for context in contexts {
            for word in 0..=sources / 32 {
                write(base, 0x2000 + context * 0x80 + word * 4, 0);
            }
            write(base, 0x200000 + context * 0x1000, 0);
        }
        for line in lines {
            write(base, line as usize * 4, 1);
        }
    }

    pub(crate) fn irq(&self, line: u32) -> Irq {
        Irq {
            controller: self.physical,
            line,
        }
    }

    fn claim_register(&self) -> usize {
        0x200004 + self.context * 0x1000
    }

    fn enable_register(&self, line: u32) -> usize {
        0x2000 + self.context * 0x80 + (line as usize / 32) * 4
    }
}

impl Controller for Plic {
    fn claim(&self) -> Option<impl Claim + '_> {
        let line = read(self.base, self.claim_register());
        (line != 0).then_some(PlicClaim { plic: self, line })
    }

    fn unmask(&self, irq: Irq) {
        assert_eq!(irq.controller, self.physical);
        let offset = self.enable_register(irq.line);
        write(
            self.base,
            offset,
            read(self.base, offset) | (1 << (irq.line % 32)),
        );
    }
}

#[must_use = "finish the claim before leaving the interrupt handler"]
struct PlicClaim<'a> {
    plic: &'a Plic,
    line: u32,
}

impl Claim for PlicClaim<'_> {
    fn finish(self) -> Irq {
        let offset = self.plic.enable_register(self.line);
        let base = self.plic.base;
        // PLIC ignores completion for a disabled source. Local interrupts
        // stay masked across both writes, so delivery cannot reenter here.
        write(base, self.plic.claim_register(), self.line);
        write(base, offset, read(base, offset) & !(1 << (self.line % 32)));
        self.plic.irq(self.line)
    }
}

fn order() {
    unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) };
}

fn read(base: usize, offset: usize) -> u32 {
    order();
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn write(base: usize, offset: usize, value: u32) {
    order();
    unsafe { ((base + offset) as *mut u32).write_volatile(value) };
    order();
}
