use ::core::ops;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub base: usize,
    pub size: usize,
}

impl Region {
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }
    pub fn range(&self) -> ops::Range<usize> {
        self.base..self.base + self.size
    }
}
