#![allow(dead_code, unused_imports)]
pub(crate) mod deadlock;
#[path = "../../../../../../src/metal/riscv/sync/global.rs"]
pub(crate) mod global;
#[path = "../../../../../../src/metal/riscv/sync/legacy.rs"]
pub(crate) mod legacy;
#[path = "../../../../../../src/metal/riscv/sync/lock/mod.rs"]
pub(crate) mod lock;
#[path = "../../../../../../src/metal/riscv/sync/once.rs"]
pub(crate) mod once;
#[path = "../../../../../../src/metal/riscv/sync/order/mod.rs"]
pub(crate) mod order;
#[path = "../../../../../../src/metal/riscv/sync/reentrant/mod.rs"]
pub(crate) mod reentrant;
#[path = "../../../../../../src/metal/riscv/sync/root.rs"]
pub(crate) mod root;
#[path = "../../../../../../src/metal/riscv/sync/spin/mod.rs"]
pub(crate) mod spin;
pub(crate) mod trap;
#[path = "../../../../../../src/metal/riscv/sync/turn.rs"]
mod turn;
pub(crate) use global::global_commit;
#[cfg(debug_assertions)]
pub(crate) use global::global_held_by_current;
pub(crate) use once::OnceLock;
pub(crate) use root::Root;
#[cfg(test)]
#[path = "../../atomic.rs"]
mod atomic;
#[cfg(debug_assertions)]
macro_rules! deadlock_caller {
    () => {
        0usize
    };
}
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
