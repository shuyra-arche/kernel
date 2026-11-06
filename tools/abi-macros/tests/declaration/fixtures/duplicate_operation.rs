#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,output=(),infallible)]
    #[operation(slot = 1)]
    pub struct A;
}
fn main() {}
