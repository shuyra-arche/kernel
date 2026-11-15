#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,output=(),infallible,reply=4)]
    pub struct A;
}
fn main() {}
