use crate::core::work::mail::{Mail, nole::NoleMeta};
use crate::core::work::pend;
use crate::entry::boot::resources::Registry;
use crate::machine::sync::OnceLock;
use alloc::sync::Arc;
use kernel_api::{
    abi::call::pie::PieFail, boot::ledger::Call, boot::ledger::Name, model::identity::Mark,
    model::identity::TaskId, model::permission::Permission,
};

pub(crate) struct Resources {
    pub build: Arc<NoleMeta>,
    pub doom: Arc<NoleMeta>,
    pub device: Arc<NoleMeta>,
}
static RESOURCES: OnceLock<Resources> = OnceLock::new();

pub(crate) fn register(registry: &mut Registry) -> Result<(), PieFail> {
    for (call, meta) in [
        (Call::Build, &get().build),
        (Call::Doom, &get().doom),
        (Call::Device, &get().device),
    ] {
        let root = pend::Pie::try_new(
            Mail::Nole(meta.clone()),
            Mark::NONE,
            Permission::FETCH | Permission::VEST,
            None,
        )?;
        registry
            .register(Name::Call(call), root)
            .map_err(|e| e.into_parts().0)?;
    }
    Ok(())
}
pub(crate) fn init() -> Result<(), PieFail> {
    if RESOURCES.get().is_some() {
        return Ok(());
    }
    let resources = Resources {
        device: NoleMeta::try_new(TaskId::new(0)).map_err(|_| PieFail::OoM)?,
        build: NoleMeta::try_new(TaskId::new(0)).map_err(|_| PieFail::OoM)?,
        doom: NoleMeta::try_new(TaskId::new(0)).map_err(|_| PieFail::OoM)?,
    };
    assert!(
        RESOURCES.set(resources).is_ok(),
        "call resources already initialized"
    );
    Ok(())
}
pub(crate) fn get() -> &'static Resources {
    RESOURCES.get().expect("call resources not initialized")
}
