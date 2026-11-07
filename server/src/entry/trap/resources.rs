use crate::core::work::pend;
use crate::core::work::room::interrupt;
use crate::core::work::view::View;
use crate::entry::boot::resources::Registry;
use kernel_api::{
    abi::call::pend::PendFail, boot::ledger::Name, boot::ledger::Trap, model::identity::Mark,
    model::modifiable::Modifiable,
};

pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    let resources = interrupt::get();
    let modifiable = Modifiable::No;
    let fault = pend::PendEntry::try_new(
        View::Clone(resources.page_fault.clone()),
        Mark::NONE,
        modifiable,
        None,
    )?;
    registry
        .register(Name::Trap(Trap::PageFault), fault)
        .map_err(|e| e.into_parts().0)?;
    Ok(())
}
