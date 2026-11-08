#[cfg(debug_assertions)]
use core::sync::atomic::{AtomicU64, Ordering};

use riscv::register::{scounteren, time};
use sbi::{TimerCall, ecall::SArgs, fid::Timer};

use crate::metal::generic::clock::{Comparator, Compare};

pub(crate) enum Backend {
    Sbi,
    Sstc,
}

impl Comparator for Backend {
    type Error = erra::Error<sbi::ecall::SError>;

    fn program(&self, setting: Compare) -> Result<(), Self::Error> {
        let at = setting.ticks();
        #[cfg(debug_assertions)]
        let start = crate::metal::clock::now().as_ticks();
        match self {
            Self::Sbi => {
                TimerCall::new(Timer::SetTimer)
                    .args(SArgs {
                        a0: at as usize,
                        ..Default::default()
                    })
                    .call()?;
            }
            Self::Sstc => {
                // The chosen backend has firmware permission on this Hart.
                unsafe { core::arch::asm!("csrw 0x14d, {at}", at = in(reg) at, options(nostack)) };
            }
        }
        #[cfg(debug_assertions)]
        {
            let spent = crate::metal::clock::now().as_ticks().saturating_sub(start);
            let (writes, ticks) = &MEASURES[usize::from(matches!(self, Self::Sstc))];
            writes.fetch_add(1, Ordering::Relaxed);
            ticks.fetch_add(spent, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[cfg(debug_assertions)]
static MEASURES: [(AtomicU64, AtomicU64); 2] =
    [const { (AtomicU64::new(0), AtomicU64::new(0)) }; 2];

#[cfg(debug_assertions)]
pub(crate) fn measures() -> [(u64, u64); 2] {
    MEASURES.each_ref().map(|(writes, ticks)| {
        (
            writes.load(Ordering::Relaxed),
            ticks.load(Ordering::Relaxed),
        )
    })
}

pub(crate) fn ticks() -> u64 {
    time::read() as u64
}

pub(crate) fn enable_user_time() -> bool {
    unsafe { scounteren::set_tm() };
    scounteren::read().tm()
}
