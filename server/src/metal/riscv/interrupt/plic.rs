use crate::metal::generic::interrupt::{Delivery, Irq};

/// Access to one S-mode context of an exclusively owned PLIC. The route serializes
/// MMIO with interrupts masked; claims/completions run on its owning Hart.
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
        for line in lines.into_iter().filter(|line| *line != 0) {
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

impl Delivery for Plic {
    fn claim_masked(&self) -> Option<Irq> {
        let line = read(self.base, self.claim_register());
        if line == 0 {
            return None;
        }
        let irq = self.irq(line);
        self.mask(irq);
        Some(irq)
    }
    fn complete(&self, irq: Irq) {
        // PLIC 1.0 §7: completion of a source disabled on this context is ignored.
        // The router enables it immediately before this write, under commit.
        write(self.base, self.claim_register(), irq.line);
    }
    fn mask(&self, irq: Irq) {
        let offset = self.enable_register(irq.line);
        write(
            self.base,
            offset,
            read(self.base, offset) & !(1 << (irq.line % 32)),
        );
    }
    fn unmask(&self, irq: Irq) {
        assert_eq!(irq.controller, self.physical);
        let offset = self.enable_register(irq.line);
        write(
            self.base,
            offset,
            read(self.base, offset) | (1 << (irq.line % 32)),
        );
        // QEMU 10.0's SiFive PLIC does not recompute its output on enable writes.
        // Rewrite the unchanged threshold so an interrupt asserted while masked
        // becomes visible too. This is also a valid, non-claiming hardware write:
        // it preserves the context's policy and never completes an unclaimed IRQ.
        let threshold = 0x200000 + self.context * 0x1000;
        write(self.base, threshold, read(self.base, threshold));
    }
    fn prioritize(&self, irq: Irq) {
        write(self.base, irq.line as usize * 4, 1);
    }
}
