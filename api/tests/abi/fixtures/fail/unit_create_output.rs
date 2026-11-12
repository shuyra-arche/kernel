use kernel_api::abi::call::unit::operations::Create;
use kernel_api::abi::operation::Operation;
use kernel_api::model::unit::UnitTarget;
fn discards_nest<C: Operation<Output = UnitTarget>>() {}
fn main() {
    discards_nest::<Create>();
}
