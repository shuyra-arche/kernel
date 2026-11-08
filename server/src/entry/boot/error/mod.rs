use crate::metal::riscv::sbi;
use ::core::fmt;
use ::core::fmt::Write;

use crate::basis::nest::NestError;
use crate::metal::allocator::InitError;
use crate::metal::clock::ClockError;
use crate::metal::console::Sink;
use crate::metal::hart::HartId;
use crate::metal::mmu::mode::SatpError;

mod map_operation;
mod resource_operation;

pub use map_operation::MapOperation;
pub use resource_operation::ResourceOperation;

const TITLE: &str = "shuyra: boot failed";

#[derive(Debug)]
pub enum BootError {
    Description(&'static str),
    Allocator(erra::Error<InitError>),
    PagingMode(SatpError),
    KernelSpace(erra::Error<NestError>),
    Clock(ClockError),
    Scheduler(NestError),
    TrapStacks(NestError),
    #[cfg(debug_assertions)]
    Dependencies(crate::metal::sync::DepInitError),
    InvalidEntryImage {
        base: usize,
        size: usize,
    },
    Mapping {
        operation: MapOperation,
        source: NestError,
    },
    Resources {
        operation: ResourceOperation,
        source: kernel_api::abi::call::pend::PendFail,
    },
    Bootstrap(NestError),
    BootstrapRelease {
        task: kernel_api::model::identity::TaskId,
        source: kernel_api::abi::call::unit::UnitFail,
    },
    HartStart {
        hart: HartId,
        source: erra::Error<sbi::ecall::SError>,
    },
}

impl BootError {
    fn fields(&self, out: &mut impl Write) -> fmt::Result {
        macro_rules! field {
            ($key:literal, $($value:tt)*) => {
                writeln!(out, "  {:<10} {}", $key, format_args!($($value)*))?
            };
        }
        macro_rules! reason {
            ($operation:literal, $source:expr) => {{
                field!("operation", $operation);
                field!("reason", "{}", $source);
            }};
        }
        match self {
            Self::Description(reason) => {
                field!("operation", "admit boot description");
                field!("reason", "{reason}");
            }
            Self::Allocator(source) => {
                field!("operation", "initialize allocator");
                field!("context", "{}", source.context());
                field!("reason", "{}", source.source);
            }
            Self::PagingMode(source) => reason!("select page table mode", source),
            Self::KernelSpace(source) => {
                field!("operation", "initialize kernel space");
                field!("context", "{}", source.context());
                field!("reason", "{}", source.source);
            }
            Self::Clock(source) => reason!("initialize clock", source),
            Self::Scheduler(source) => reason!("initialize scheduler", source),
            Self::TrapStacks(source) => reason!("initialize trap stacks", source),
            #[cfg(debug_assertions)]
            Self::Dependencies(source) => {
                field!("operation", "initialize lock dependencies");
                field!(
                    "reason",
                    "{}",
                    match source {
                        crate::metal::sync::DepInitError::OutOfMemory => "out of memory",
                        crate::metal::sync::DepInitError::AlreadyInit => "already initialized",
                    }
                );
            }
            Self::InvalidEntryImage { base, size } => {
                field!("operation", "read boot entry");
                field!("initrd", "{base:#x}");
                field!("size", "{size} bytes");
                field!("reason", "invalid entry descriptor or capsule range");
            }
            Self::Mapping { operation, source } => {
                field!("operation", "{}", operation.label());
                field!("reason", "{source}");
            }
            Self::Resources { operation, source } => {
                field!("operation", "{}", operation.label());
                field!("reason", "{source}");
            }
            Self::Bootstrap(source) => reason!("create bootstrap task", source),
            Self::BootstrapRelease { task, source } => {
                field!("operation", "release bootstrap task");
                field!("task", "{}", task.get());
                field!("reason", "{source}");
            }
            Self::HartStart { hart, source } => {
                field!("operation", "start hart");
                field!("hart", "{hart}");
                field!("context", "{}", source.context());
                field!("reason", "{}", source.source);
            }
        }
        Ok(())
    }

    fn print(&self, sink: &mut impl Write) {
        let _ = writeln!(sink, "{TITLE}\n");
        let _ = self.fields(sink);
        let _ = writeln!(sink);
    }
}

pub fn fail(error: BootError) -> ! {
    crate::metal::stop::stop_boot();
    error.print(&mut Sink);
    crate::metal::stop::failed()
}
