use kernel_api::abi::call::cell::operations::Create;
use kernel_api::abi::operation::Operation;
use kernel_api::model::identity::PendId;
fn creates_authority<C: Operation<Output = PendId>>() {}
fn main() {
    creates_authority::<Create>();
}
