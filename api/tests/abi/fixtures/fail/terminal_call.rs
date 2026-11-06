use kernel_api::abi::{call::room::operations::Reap, operation::KernelCall};
fn returning<C: KernelCall>() {}
fn main() {
    returning::<Reap>();
}
