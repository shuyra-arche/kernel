#![no_std]
#![forbid(unsafe_code)]
//! Kernel contracts: shared model, call ABI and initial-task boot protocol.
//! Client executes these contracts; Server implements them.

pub mod abi;
pub mod boot;
pub mod model;

pub mod address;
