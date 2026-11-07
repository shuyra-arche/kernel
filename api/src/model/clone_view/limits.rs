#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloneLimits {
    pub max_len: usize,
    pub max_messages: usize,
    pub max_bytes: usize,
}

impl Default for CloneLimits {
    fn default() -> Self {
        Self {
            max_len: isize::MAX as usize,
            max_messages: 4,
            max_bytes: usize::MAX,
        }
    }
}

impl CloneLimits {
    pub fn valid(self) -> bool {
        self.max_len > 0
            && self.max_len <= isize::MAX as usize
            && self.max_messages > 0
            && self.max_bytes >= self.max_len
    }
}
