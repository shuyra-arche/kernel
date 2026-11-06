#[kernel_api_macros::envcall]
mod definition {
    #[error_domain]
    pub enum DispatchFail {
        Unknown = -1,
    }
}
pub use definition::DispatchFail;
