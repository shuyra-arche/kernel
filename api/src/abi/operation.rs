//! Kernel operations and their register ABI, independent of execution.

/// A returning operation owns its success and failure types; its value is input.
/// A release operation cannot be assigned a different success type.
pub trait Operation {
    type Output;
    type Error;
}

/// A request's register representation. Both returning and terminal calls use it.
/// Implementations must keep `pack` and `from_registers` consistent with the ABI.
/// A declaration checks the sum of field widths at compile time.
pub trait RegisterCall: Sized {
    const SLOT: usize;
    fn pack(&self) -> [usize; 6];
    fn from_registers(registers: &[usize; 6]) -> Result<Self, crate::abi::register::Decode>;
}

/// A returning Kernel operation with pure result decoding.
/// No trap, retry, rollback or runtime state is implied by this contract.
pub trait KernelCall: Operation + RegisterCall {
    fn decode_return(registers: (usize, usize, usize)) -> Result<Self::Output, Self::Error>;
}

/// A Kernel call that must not return. It has no success/error decoder and cannot
/// be passed to a returning invocation. Client owns the unexpected-return fault.
pub trait TerminalCall: RegisterCall {}
