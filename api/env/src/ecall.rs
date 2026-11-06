//! Kernel operations and their register ABI, independent of execution.

/// A returning operation owns its success and failure types; its value is input.
/// A release operation cannot be assigned a different success type:
///
/// ```compile_fail,E0271
/// use kernel_api_env::{Operation, abi::call::memory::operations::Release};
/// fn returns_number<C: Operation<Output = usize>>() {}
/// returns_number::<Release>();
/// ```
pub trait Operation {
    type Output;
    type Error;
}

/// A request's register representation. Both returning and terminal calls use it.
/// Implementations must keep `pack` and `from_registers` consistent with the ABI.
/// A declaration checks the sum of field widths at compile time:
///
/// ```standalone_crate
/// use kernel_api_env::{ecall, wire, RegisterCall};
/// #[kernel_api_macros::envcall(class = 42)]
/// mod calls {
///     #[operation(slot = 7, output = usize, infallible)]
///     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
///     pub struct Read { pub registers: usize, pub cursor: usize }
/// }
/// fn main() {
///     let input = calls::Read { registers: 10, cursor: 20 };
///     assert_eq!(input.pack(), [10, 20, 0, 0, 0, 0]);
///     assert_eq!(calls::Read::from_registers(&input.pack()).unwrap(), input);
/// }
/// ```
///
/// ```compile_fail,E0080
/// use kernel_api_env::{ecall, wire};
/// #[kernel_api_macros::envcall(class = 42)]
/// mod calls {
///     #[operation(slot = 7, output = (), infallible)]
///     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
///     pub struct Oversized {
///         pub a: usize, pub b: usize, pub c: usize, pub d: usize,
///         pub e: usize, pub f: usize, pub g: usize,
///     }
/// }
/// fn main() {}
/// ```
pub trait RegisterCall: Sized {
    const SLOT: usize;
    fn pack(&self) -> [usize; 6];
    fn from_registers(registers: &[usize; 6]) -> Result<Self, crate::wire::Decode>;
}

/// A returning Kernel operation with pure result decoding.
/// No trap, retry, rollback or runtime state is implied by this contract.
pub trait KernelCall: Operation + RegisterCall {
    fn decode_return(registers: (usize, usize, usize)) -> Result<Self::Output, Self::Error>;
}

/// A Kernel call that must not return. It has no success/error decoder and cannot
/// be passed to a returning invocation. Client owns the unexpected-return fault.
///
/// ```compile_fail,E0277
/// use kernel_api_env::{KernelCall, abi::call::room::operations::Reap};
/// fn returning<C: KernelCall>() {}
/// returning::<Reap>();
/// ```
pub trait TerminalCall: RegisterCall {}

pub trait FailCode: Copy + core::fmt::Debug {
    fn code(self) -> isize;
}

pub fn make_fail<F: FailCode + core::fmt::Display>(f: F) -> erra::Error<F> {
    erra::Error::new("envcall", f)
}
