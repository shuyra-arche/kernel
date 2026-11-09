//! No address translation, heap allocation, or Kernel lock. The firmware call
//! can block: the byte budget bounds Kernel work, not firmware latency.
use core::fmt::{self, Write};

pub struct Emergency<F> {
    emit: F,
    remaining: usize,
    lost: usize,
    failed: bool,
}
impl<F: FnMut(u8) -> bool> Emergency<F> {
    pub fn with(emit: F) -> Self {
        Self {
            emit,
            remaining: 8192,
            lost: 0,
            failed: false,
        }
    }
    pub fn finish(mut self) {
        let lost = self.lost;
        // One bounded best-effort footer, even after an earlier output error.
        self.remaining = 128;
        self.failed = false;
        let _ = writeln!(self, "\n[kernel] emergency-lost={lost}");
    }
}
impl<F: FnMut(u8) -> bool> Write for Emergency<F> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for (index, byte) in text.bytes().enumerate() {
            if self.remaining == 0 || self.failed {
                self.lost = self.lost.saturating_add(text.len() - index);
                return Err(fmt::Error);
            }
            self.remaining -= 1;
            if !(self.emit)(byte) {
                self.failed = true;
                self.lost = self.lost.saturating_add(text.len() - index);
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}
