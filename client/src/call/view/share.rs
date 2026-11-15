use kernel_api::{
    abi::call::share_view::{ShareFail, operations as op},
    address::VirtAddr,
    model::identity::PendId,
};
typed_calls! {
    fallible fn delete(token: PendId) -> Result<(), erra::Error<ShareFail>> = op::Delete { token };
    fallible fn derive(license: PendId, request: VirtAddr) -> Result<PendId, erra::Error<ShareFail>> = op::Derive { license, request };
}
