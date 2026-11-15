#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,output=(),error=Fail,infallible)]
    pub struct A;
}
fn main() {}
