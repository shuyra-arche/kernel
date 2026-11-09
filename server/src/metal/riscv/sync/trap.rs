use riscv::register::sstatus;

pub(crate) struct TrapGuard {
    sie_was_enabled: bool,
}

impl TrapGuard {
    #[inline(always)]
    pub(crate) unsafe fn save() -> Self {
        unsafe {
            let was = sstatus::read().sie();
            if sstatus::read().sie() {
                sstatus::clear_sie();
            }
            crate::metal::interrupt::window::enter();
            #[cfg(feature = "latency")]
            if was {
                crate::metal::measure::resume_irq(true);
            }
            TrapGuard {
                sie_was_enabled: was,
            }
        }
    }
}

impl Drop for TrapGuard {
    #[inline(always)]
    fn drop(&mut self) {
        crate::metal::interrupt::window::leave();
        if self.sie_was_enabled {
            #[cfg(feature = "latency")]
            let _ = crate::metal::measure::pause_irq();
            unsafe {
                sstatus::set_sie();
            }
        }
    }
}
