use kernel_api::{
    abi::call::nest::{NestResult, operations},
    address::VirtAddr,
    model::identity::PendId,
};
typed_calls! {
    fallible fn create(license:PendId) -> NestResult<PendId> = operations::Create { license };
    fallible fn inspect(token:PendId,out:VirtAddr) -> NestResult<()> = operations::Inspect { token,out };
    fallible fn delete(token:PendId) -> NestResult<()> = operations::Delete { token };
}
