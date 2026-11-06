use kernel_api::abi;
#[kernel_api_macros::envcall(class = 42)]
mod calls {
    #[operation(slot = 7, output = (), infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Oversized {
        pub a: usize,
        pub b: usize,
        pub c: usize,
        pub d: usize,
        pub e: usize,
        pub f: usize,
        pub g: usize,
    }
}
fn main() {}
