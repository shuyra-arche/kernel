//! Typed effects generated from the canonical API declaration.

pub use env::*;

env::__kernel_client_chrono!(crate::ecall::trap);
env::__kernel_client_control!(crate::ecall::trap);
env::__kernel_client_debug!(crate::ecall::trap);
env::__kernel_client_hole!(crate::ecall::trap);
env::__kernel_client_memory!(crate::ecall::trap);
env::__kernel_client_nole!(crate::ecall::trap);
env::__kernel_client_pie!(crate::ecall::trap);
env::__kernel_client_pole!(crate::ecall::trap);
env::__kernel_client_room!(crate::ecall::trap);
env::__kernel_client_tole!(crate::ecall::trap);
env::__kernel_client_unit!(crate::ecall::trap);
