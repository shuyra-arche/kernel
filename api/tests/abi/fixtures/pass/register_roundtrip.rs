use kernel_api::abi;
use kernel_api::abi::operation::RegisterCall;
#[kernel_api_macros::envcall(class = 42)]
mod calls {
    #[operation(slot = 7, output = usize, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Read {
        pub registers: usize,
        pub cursor: usize,
    }
}
fn main() {
    let input = calls::Read {
        registers: 10,
        cursor: 20,
    };
    assert_eq!(input.pack(), [10, 20, 0, 0, 0, 0]);
    assert_eq!(calls::Read::from_registers(&input.pack()).unwrap(), input);
}
