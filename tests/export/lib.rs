#![allow(dead_code, unused_imports)]

extern crate alloc;
extern crate self as semihosting;

pub use std::io;
#[path = "../../src/core/memory/nest/fault.rs"]
mod fault;
#[path = "fixture/file.rs"]
pub mod fs;
#[path = "fixture/machine.rs"]
mod machine;
#[path = "../../src/entry/report/report/mod.rs"]
mod report;

mod core {
    pub mod memory {
        pub mod nest {
            pub use crate::fault::FaultKind;
        }
    }
}

mod entry {
    pub mod report {
        pub(crate) use crate::report;
    }
}

#[macro_export]
macro_rules! putln {
    ($($arg:tt)*) => { println!($($arg)*) };
}

mod export {
    include!("../../src/entry/report/export.rs");

    #[cfg(test)]
    mod tests {
        include!("cases.rs");
    }
}
