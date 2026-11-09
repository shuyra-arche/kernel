use riscv::register::sstatus;

pub(crate) struct TrapGuard {
    sie_was_enabled: bool,
    #[cfg(feature = "latency")]
    start: u64,
}

impl TrapGuard {
    #[inline(always)]
    pub(crate) unsafe fn save() -> Self {
        unsafe {
            let was = sstatus::read().sie();
            if sstatus::read().sie() {
                sstatus::clear_sie();
            }
            TrapGuard {
                sie_was_enabled: was,
                #[cfg(feature = "latency")]
                start: if was {
                    crate::metal::measure::stamp()
                } else {
                    0
                },
            }
        }
    }
}

impl Drop for TrapGuard {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(feature = "latency")]
        crate::metal::measure::elapsed(crate::metal::measure::Metric::IrqOff, self.start);
        if self.sie_was_enabled {
            unsafe {
                sstatus::set_sie();
            }
        }
    }
}
