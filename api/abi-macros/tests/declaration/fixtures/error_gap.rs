#[kernel_api_macros::envcall(class = 2)]
mod declaration {
    #[error_domain]
    pub enum Failure {
        Denied = -2,
    }
    #[operation(slot=0,output=(),error=Failure)]
    pub struct Release;
}
fn main() {}
