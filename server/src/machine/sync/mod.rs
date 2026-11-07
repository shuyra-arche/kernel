mod depend;
mod once;
mod pair;
pub(crate) mod reentrant;
mod serial;
pub(crate) mod spin;
mod trap;

pub(crate) use pair::with_pair;
pub(crate) use serial::next_id;

#[cfg(debug_assertions)]
macro_rules! depend_enter {
    ($lock:expr) => {{
        let caller: usize;
        unsafe { ::core::arch::asm!("mv {0}, ra", out(reg) caller) };
        caller
    }};
}

macro_rules! depend_check {
    ($lock:expr, $caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::machine::sync::depend::check(
            $lock as *const _ as *const () as usize,
            $lock.level,
            $caller,
        );
    }};
}

macro_rules! depend_acquire {
    ($lock:expr, $caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::machine::sync::depend::acquire(
            $lock as *const _ as *const () as usize,
            $lock.level,
            $caller,
        );
    }};
}

macro_rules! depend_check_peer {
    ($lock:expr, $lower:expr, $caller:expr) => {{
        #[cfg(debug_assertions)]
        crate::machine::sync::depend::check_peer(
            $lock as *const _ as *const () as usize,
            $lower as *const _ as *const () as usize,
            $lock.level,
            $caller,
        );
    }};
}

macro_rules! depend_release {
    ($lock:expr) => {{
        #[cfg(debug_assertions)]
        crate::machine::sync::depend::release($lock as *const _ as *const () as usize);
    }};
}
pub(crate) use depend_acquire;
pub(crate) use depend_check;
pub(crate) use depend_check_peer;
#[cfg(debug_assertions)]
pub(crate) use depend_enter;
pub(crate) use depend_release;

#[cfg(debug_assertions)]
pub use depend::DepInitError;
pub use depend::Level;
pub(crate) use depend::reserve as reserve_depend;
pub use once::OnceLock;
pub use reentrant::RelLock;
pub use spin::SpinLock;
pub(crate) use spin::SpinLockGuard;

#[cfg(debug_assertions)]
pub fn init_depend(hart_count: usize) -> Result<(), depend::DepInitError> {
    depend::init(hart_count)
}
