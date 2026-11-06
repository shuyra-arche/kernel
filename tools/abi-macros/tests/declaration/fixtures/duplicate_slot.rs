#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,output=(),infallible)]
    pub struct A;
    #[operation(slot=0,output=(),infallible)]
    pub struct B;
}
fn main() {}
