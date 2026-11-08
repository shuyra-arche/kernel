use kernel_api::abi::call::unit::operations::Exit;
use kernel_api::abi::operation::KernelCall;
fn returning<C: KernelCall>() {}
fn main() {
    returning::<Exit>();
}
