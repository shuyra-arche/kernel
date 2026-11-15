#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(output=(),infallible)]
    pub struct A;
}
fn main() {}
