use alloc::vec::Vec;

/// A finite pool of reusable numbers. Release validates the allocation bit;
/// the caller must own the live lease and first discharge all external users.
pub(crate) struct Recycle {
    first: usize,
    count: usize,
    bits: Vec<u64>,
}

impl Recycle {
    pub(crate) const fn new(first: usize, end: usize) -> Self {
        assert!(first < end);
        Self {
            first,
            count: end - first,
            bits: Vec::new(),
        }
    }

    pub(crate) fn allocate(&mut self) -> Option<usize> {
        if self.bits.is_empty() {
            let words = self.count.div_ceil(64);
            self.bits.try_reserve_exact(words).ok()?;
            self.bits.resize(words, 0);
        }
        for (word, bits) in self.bits.iter_mut().enumerate() {
            let bit = (!*bits).trailing_zeros() as usize;
            let offset = word * 64 + bit;
            if bit < 64 && offset < self.count {
                *bits |= 1 << bit;
                return Some(self.first + offset);
            }
        }
        None
    }

    pub(crate) fn release(&mut self, id: usize) -> bool {
        let Some(offset) = id.checked_sub(self.first).filter(|n| *n < self.count) else {
            return false;
        };
        let Some(bits) = self.bits.get_mut(offset / 64) else {
            return false;
        };
        let mask = 1 << (offset % 64);
        if *bits & mask == 0 {
            return false;
        }
        *bits &= !mask;
        true
    }
}
