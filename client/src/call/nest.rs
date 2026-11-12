use kernel_api::{
    abi::call::nest::{NestResult, operations},
    address::{PageCount, VirtAddr},
    model::{
        cell::{CellLevel, Placement},
        identity::PendId,
        nest::ReserveRequest,
    },
};
typed_calls! {
    fallible fn create(license:PendId) -> NestResult<PendId> = operations::Create { license };
    fallible fn inspect(token:PendId,out:VirtAddr) -> NestResult<()> = operations::Inspect { token,out };
    fallible fn delete(token:PendId) -> NestResult<()> = operations::Delete { token };
    fallible fn release(token:PendId) -> NestResult<()> = operations::Release { token };
}

/// Reserve vacant mapping positions without materializing or publishing a Page.
#[inline]
pub fn reserve(
    token: PendId,
    placement: Placement,
    count: PageCount,
    level: CellLevel,
    access: u64,
) -> NestResult<(PendId, VirtAddr)> {
    let snapshot = ReserveRequest {
        placement,
        count,
        level,
        access,
    }
    .to_bytes();
    unsafe {
        crate::invoke::invoke(operations::Reserve {
            token,
            request: VirtAddr::new(snapshot.as_ptr() as usize),
        })
    }
}
