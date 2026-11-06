#[kernel_api_macros::envcall()]
mod declaration {
    #[operation(slot=0,output=(),infallible)]
    pub struct A;
}
fn main() {}
