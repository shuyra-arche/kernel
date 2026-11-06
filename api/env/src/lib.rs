#![no_std]
//! Kernel register operations, capability vocabulary and concrete ABI layouts.
//!
//! Each domain declares operations and failures together through `envcall`.
//! Request values carry inputs; `Operation` owns success/error types, and
//! `KernelCall` adds pure return decoding. Aggregate request enums serve Server
//! dispatch. Client owns named execution entry points and the native trap.
//!
//! Byte formats are limited to Kernel memory descriptors and startup records.
//! Generic service messages and protocol generation belong to consumers.
extern crate alloc;
extern crate self as env;

pub mod abi;
pub mod ecall;
pub mod ledger;
pub mod wire;

pub use abi::call::hole::{HoleCall, HoleCondition, HoleFail, HoleResult};
pub use abi::call::memory::PAGE_SIZE;
pub use abi::call::nole::{NoleCall, NoleFail, NoleResult};
pub use abi::call::pole::{PoleCall, PoleCondition, PoleFail, PoleResult};
pub use abi::call::tole::{
    ToleAwaitReply, ToleCall, ToleEventFail, ToleEventSource, ToleFail, ToleResult,
};
pub use abi::call::{
    AcquireSource, Allocation, Bit, ChronoCall, ControlCall, ControlFail, ControlResult, DBCN_MAX,
    DebugCall, DebugFail, DebugResult, DispatchFail, EnvCall, GrantRange, HoleLimits, MemoryCall,
    MemoryFail, MemoryResult, NOTE_MAX, Oversize, PieCall, PieFail, PieInfo, PieResult, Placement,
    PlacementAt, PlacementError, PullOutcome, ReferenceRange, ReleaseMode, RoomCall, RoomFail,
    RoomResult, UnitCall, UnitFail, UnitResult, ViewControl, ViewSelection,
};
pub use abi::exit::{EXIT_FAULT, EXIT_OK, EXIT_PANIC, Reason};
pub use abi::permission::{Access, Permission, Policy};
pub use abi::wait::Wait;
pub use ecall::{FailCode, KernelCall, Operation, RegisterCall, TerminalCall, make_fail};
pub use wire::pie_kind::PieKind;
pub use wire::program_kind::ProgramKind;
pub use wire::{
    Decode, FlatReference, FromPair, Mark, PieToken, TaskId, TeamId, ViewId, VirtAddr, Wire,
};

pub use ledger::entry::{ENTRY_LEN, Entry};
pub use ledger::name::{Call, NAME_LEN, Name, Page, Trap};

pub use abi::call::unit::{ExitCause, JoinReply, TaskExit, UnitTarget};
