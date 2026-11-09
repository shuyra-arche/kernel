//! No address translation, heap allocation, or Kernel lock. The firmware call
//! can block: the byte budget bounds Kernel work, not firmware latency.
use super::super::sbi::{Dbcn, DbcnCall, ecall::SArgs};
use core::fmt::{self, Write};

pub struct Emergency {
    remaining: usize,
    lost: usize,
    failed: bool,
}
impl Emergency {
    pub fn new() -> Self {
        Self {
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
impl Write for Emergency {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for (index, byte) in text.bytes().enumerate() {
            if self.remaining == 0 || self.failed {
                self.lost = self.lost.saturating_add(text.len() - index);
                return Err(fmt::Error);
            }
            self.remaining -= 1;
            if DbcnCall::new(Dbcn::ConsoleWriteByte)
                .args(SArgs {
                    a0: byte as usize,
                    ..Default::default()
                })
                .call()
                .is_err()
            {
                self.failed = true;
                self.lost = self.lost.saturating_add(text.len() - index);
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}
