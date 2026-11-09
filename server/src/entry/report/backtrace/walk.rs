//! Bounded best-effort stack evidence. A scan hit is never a proven caller.
pub const DEPTH: usize = 32;
pub const SPAN: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    FrameChain,
    StackScan,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    End,
    Bounds,
    Unreadable,
    NonMonotonic,
    InvalidPc,
    Limit,
}
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub pc: usize,
    pub sp: usize,
    pub source: Source,
}
pub struct Backtrace {
    frames: [Frame; DEPTH],
    count: usize,
    pub stop: Stop,
}
impl Backtrace {
    pub fn frames(&self) -> &[Frame] {
        &self.frames[..self.count]
    }
    pub fn walk(
        mut word: impl FnMut(usize) -> Option<usize>,
        sp: usize,
        fp: usize,
        ceiling: usize,
        code: impl Fn(usize) -> bool,
    ) -> Self {
        let mut trace = Self {
            frames: [Frame {
                pc: 0,
                sp: 0,
                source: Source::FrameChain,
            }; DEPTH],
            count: 0,
            stop: Stop::Bounds,
        };
        let ceiling = ceiling.min(sp.saturating_add(SPAN));
        let mut f = fp;
        // Bound attempts as well as stored frames, even if every PC is corrupt.
        for _ in 0..DEPTH {
            if f == 0 {
                trace.stop = Stop::End;
                break;
            }
            if f < sp.saturating_add(16) || f > ceiling || f % 16 != 0 {
                trace.stop = Stop::Bounds;
                break;
            }
            let (Some(caller), Some(pc)) = (word(f - 16), word(f - 8)) else {
                trace.stop = Stop::Unreadable;
                break;
            };
            if pc == 0 || pc & 1 != 0 || !code(pc) {
                trace.stop = Stop::InvalidPc;
                break;
            }
            trace.push(pc, f, Source::FrameChain);
            if caller != 0 && caller <= f {
                trace.stop = Stop::NonMonotonic;
                break;
            }
            f = caller;
            trace.stop = Stop::Limit;
        }
        // Independently bounded scan, explicitly marked heuristic. Do not read
        // beyond the selected stack window or bridge unreadable stack pages.
        let mut address = sp;
        while address.checked_add(8).is_some_and(|end| end <= ceiling) && trace.count < DEPTH {
            let Some(pc) = word(address) else {
                break;
            };
            if pc != 0 && pc & 1 == 0 && code(pc) && !trace.frames().iter().any(|f| f.pc == pc) {
                trace.push(pc, address, Source::StackScan);
            }
            address += 8;
        }
        trace
    }
    fn push(&mut self, pc: usize, sp: usize, source: Source) {
        self.frames[self.count] = Frame { pc, sp, source };
        self.count += 1;
    }
}
