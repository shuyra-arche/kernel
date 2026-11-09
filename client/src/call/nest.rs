use kernel_api::{
    abi::call::nest::{NestResult, operations},
    address::VirtAddr,
    model::{identity::PendId, nest::NestKind},
};
typed_calls! {
    fallible fn create(license:PendId,kind:NestKind) -> NestResult<PendId> = operations::Create { license,kind };
    fallible fn inspect(token:PendId,out:VirtAddr) -> NestResult<()> = operations::Inspect { token,out };
    fallible fn attach(token:PendId,port:PendId) -> NestResult<()> = operations::Attach { token,port };
    fallible fn detach(token:PendId) -> NestResult<()> = operations::Detach { token };
    fallible fn delete(token:PendId) -> NestResult<()> = operations::Delete { token };
}
