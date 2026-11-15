#[cfg(debug_assertions)]
pub(crate) mod deadlock;
mod global;
mod legacy;
pub(crate) mod lock;
mod once;
pub(crate) mod order;
mod reentrant;
mod root;
mod spin;
mod trap;
mod turn;
#[cfg(debug_assertions)]
pub use deadlock::DeadlockInitError;
pub(crate) use global::global_commit;
#[cfg(debug_assertions)]
pub(crate) use global::global_held_by_current;
pub(crate) use legacy::LegacyLock;
pub use once::OnceLock;
pub(crate) use reentrant::RelLockGuard as LegacyCommitGuard;
pub(crate) use root::Root;
pub(crate) use spin::SpinLockGuard as LegacyGuard;
pub(crate) use trap::TrapGuard;
#[cfg(debug_assertions)]
pub fn init_deadlock(hart_count: usize) -> Result<(), DeadlockInitError> {
    deadlock::init(hart_count)
}

#[cfg(debug_assertions)]
macro_rules! deadlock_caller {() => {{let caller:usize; unsafe {core::arch::asm!("mv {0}, ra",out(reg) caller)};caller}};}
macro_rules! deadlock_before {
    ($lock:expr,$caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::metal::sync::deadlock::before(
            $lock as *const _ as *const () as usize,
            $lock.class,
            $caller,
        );
    }};
}
macro_rules! deadlock_acquired {
    ($lock:expr,$caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::metal::sync::deadlock::acquired(
            $lock as *const _ as *const () as usize,
            $lock.class,
            $caller,
            crate::metal::sync::deadlock::Kind::Ordinary,
        );
    }};
}
macro_rules! deadlock_before_pair {
    ($lock:expr,$lower:expr,$pair:expr,$caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::metal::sync::deadlock::before_pair(
            $lock as *const _ as *const () as usize,
            $lower as *const _ as *const () as usize,
            $lock.class,
            $pair,
            $caller,
        );
    }};
}
macro_rules! deadlock_acquired_pair {
    ($lock:expr,$lower:expr,$caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::metal::sync::deadlock::acquired(
            $lock as *const _ as *const () as usize,
            $lock.class,
            $caller,
            crate::metal::sync::deadlock::Kind::Pair {
                predecessor: $lower as *const _ as *const () as usize,
            },
        );
    }};
}
macro_rules! deadlock_released {
    ($lock:expr) => {{
        #[cfg(debug_assertions)]
        crate::metal::sync::deadlock::released($lock as *const _ as *const () as usize);
    }};
}
#[cfg(debug_assertions)]
pub(crate) use deadlock_caller;
pub(crate) use {
    deadlock_acquired, deadlock_acquired_pair, deadlock_before, deadlock_before_pair,
    deadlock_released,
};
