#[kernel_api_macros::envcall()]
mod declaration {
    #[error_domain]
    pub enum Failure {
        #[busy]
        A = -1,
        #[busy]
        B = -2,
    }
}
fn main() {}
