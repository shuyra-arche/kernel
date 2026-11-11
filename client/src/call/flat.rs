use kernel_api::{
    abi::call::flat::{FlatResult, operations},
    address::VirtAddr,
    model::{cell::SlotId, identity::PendId},
};
typed_calls! {
    fallible fn block(token: PendId) -> FlatResult<()> = operations::Block { token };
    fallible fn allow(token: PendId) -> FlatResult<()> = operations::Allow { token };
 fallible fn create(license:PendId,size:usize,align:usize,level:usize)->FlatResult<PendId> = operations::Create{license,size,align,level};
 fallible fn inspect(token:PendId,out:VirtAddr)->FlatResult<()> = operations::Inspect{token,out};
 fallible fn slice(token:PendId,offset:usize,count:usize)->FlatResult<SlotId> = operations::Slice{token,offset,count};
 fallible fn delete(token:PendId)->FlatResult<()> = operations::Delete{token};
}
