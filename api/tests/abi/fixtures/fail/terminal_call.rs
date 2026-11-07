use kernel_api::abi::call::room::operations::Reap;
use kernel_api::abi::operation::KernelCall;
fn returning<C: KernelCall>() {}
fn main() {
    returning::<Reap>();
}
