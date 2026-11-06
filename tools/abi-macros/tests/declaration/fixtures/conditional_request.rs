#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[cfg(any())]
    #[operation(slot=0,output=(),infallible)]
    pub struct A;
}
fn main() {}
