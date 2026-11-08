//! Shared View creation and access.

use kernel_api::abi::call::share_view::operations as op;
use kernel_api::{
    abi::call::share_view::ShareFail, address::VirtAddr, model::cell::SlotId,
    model::identity::PendId, model::modifiable::Modifiable,
};

typed_calls! {
    fallible fn create(license: PendId, reference: SlotId, modifiable: Modifiable) -> Result<PendId, erra::Error<ShareFail>> =
        op::Create { license, reference, modifiable };
    fallible fn delete(token: PendId) -> Result<(), erra::Error<ShareFail>> =
        op::Delete { token };
    fallible fn open(token: PendId) -> Result<(VirtAddr, usize), erra::Error<ShareFail>> =
        op::Open { token };
    fallible fn shut(token: PendId) -> Result<(), erra::Error<ShareFail>> =
        op::Shut { token };
}
