#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=4294967296u64,output=(),infallible)]
    pub struct A;
}
fn main() {}
