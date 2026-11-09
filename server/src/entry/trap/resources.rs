use crate::basis::pend;
use crate::basis::view::CloneView;
use crate::basis::view::View;
use crate::entry::boot::resources::Registry;
use crate::metal::sync::OnceLock;
use alloc::sync::Arc;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::Trap;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    let modifiable = Modifiable::No;
    let fault = pend::PendEntry::try_new(
        View::Clone(
            PAGE_FAULT
                .get()
                .expect("trap resources initialized")
                .clone(),
        ),
        Mark::NONE,
        modifiable,
        None,
    )?;
    registry
        .register(Name::Trap(Trap::PageFault), fault)
        .map_err(|e| e.into_parts().0)?;
    Ok(())
}

static PAGE_FAULT: OnceLock<Arc<CloneView>> = OnceLock::new();
pub(crate) fn init() -> Result<(), PendFail> {
    if PAGE_FAULT.get().is_none() {
        let fault = CloneView::try_new(TaskId::new(0)).map_err(|_| PendFail::OutOfMemory)?;
        assert!(
            PAGE_FAULT.set(fault).is_ok(),
            "trap resources already initialized"
        );
    }
    Ok(())
}
