#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,slot=1,output=(),infallible)]
    pub struct A;
}
fn main() {}
