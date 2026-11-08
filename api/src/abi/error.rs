//! Common failure representation; domain failures live beside their operations.

pub trait FailCode: Copy + core::fmt::Debug {
    fn code(self) -> isize;
}

pub fn make_fail<F: FailCode + core::fmt::Display>(f: F) -> erra::Error<F> {
    erra::Error::new("envcall", f)
}

#[kernel_api_macros::envcall]
mod definition {
    #[error_domain]
    pub enum DispatchFail {
        Unknown = -1,
    }
}
pub use definition::DispatchFail;
