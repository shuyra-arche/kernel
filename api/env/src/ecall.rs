//! Pure failure vocabulary shared by ABI declarations and return decoders.

pub trait FailCode: Copy + core::fmt::Debug {
    fn code(self) -> isize;
}

pub fn make_fail<F: FailCode + core::fmt::Display>(f: F) -> erra::Error<F> {
    erra::Error::new("envcall", f)
}
