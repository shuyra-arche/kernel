use kernel_api::{
    abi::call::port::{PortResult, operations},
    address::VirtAddr,
    model::identity::PendId,
};
typed_calls! { fallible fn inspect(token: PendId, out: VirtAddr) -> PortResult<()> = operations::Inspect { token,out }; }
