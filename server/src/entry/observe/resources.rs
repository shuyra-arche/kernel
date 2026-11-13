//! Boot grants only read access and update observation, with independent Pend
//! revocation for the data and notification. No observer control is exported.
use crate::basis::{
    flat::{BorrowedRegion, Flat, sharing::Sharing},
    pend::{PendEntry, Target},
    sign::endpoint::{self, Endpoint},
    view::View,
};
use crate::entry::boot::resources::Registry;
use crate::metal::sync::OnceLock;
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pend::PendFail,
    address::PhysAddr,
    boot::ledger::{Name, Page},
    model::{
        identity::{Mark, TaskId},
        modifiable::Modifiable,
        pend::Operations,
        sign::SignRights,
    },
};
static SIGN: OnceLock<Arc<Endpoint>> = OnceLock::new();
pub(super) fn notify() {
    if let Some(sign) = SIGN.get() {
        let _ = endpoint::trigger(sign);
    }
}
pub(crate) fn identifies(sign: &Arc<Endpoint>) -> bool {
    SIGN.get().is_some_and(|known| Arc::ptr_eq(sign, known))
}
pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    // PAGE is a page-aligned static inside the reserved kernel image. Its
    // lifetime is the whole boot; no external pager owns its code or storage.
    let region =
        unsafe { BorrowedRegion::new(PhysAddr::from_raw(&super::PAGE as *const _ as usize), 4096) }
            .map_err(|_| PendFail::Denied)?;
    let flat = Flat::from_region(region).map_err(|_| PendFail::OutOfMemory)?;
    let share = Sharing::from_source(flat.whole(), 4096, TaskId::new(0))?;
    let data = PendEntry::try_new(View::Share(share), Mark::NONE, Modifiable::No, None)?;
    registry
        .register(Name::Page(Page::Memory), data)
        .map_err(|e| e.into_parts().0)?;
    let sign = Endpoint::try_new(TaskId::new(0)).map_err(|_| PendFail::OutOfMemory)?;
    SIGN.set(sign.clone()).map_err(|_| PendFail::Denied)?;
    let event = PendEntry::try_new(Target::Sign(sign), Mark::NONE, Modifiable::No, None)?;
    let mut limits = event.authority().limits();
    limits.operations = Operations::Sign(SignRights::LISTEN | SignRights::CONSUME);
    event.narrow(limits)?;
    registry
        .register(Name::Memory, event)
        .map_err(|e| e.into_parts().0)
}
