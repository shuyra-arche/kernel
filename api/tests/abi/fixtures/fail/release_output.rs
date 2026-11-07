use kernel_api::abi::call::cell::operations::Release;
use kernel_api::abi::operation::Operation;
fn returns_number<C: Operation<Output = usize>>() {}
fn main() {
    returns_number::<Release>();
}
