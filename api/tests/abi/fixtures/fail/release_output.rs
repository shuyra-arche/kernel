use kernel_api::abi::{call::memory::operations::Release, operation::Operation};
fn returns_number<C: Operation<Output = usize>>() {}
fn main() {
    returns_number::<Release>();
}
