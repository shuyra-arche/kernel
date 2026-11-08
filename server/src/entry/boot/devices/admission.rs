use crate::{entry::boot::facts, metal::sync::OnceLock};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel_api::{
    abi::call::pend::PendFail,
    boot::platform::{Region, devices::Devices},
};
pub(super) struct Description {
    pub authorities: Vec<Region>,
    pub protected: Vec<Region>,
    pub external: Vec<Region>,
    pub controllers: Vec<Region>,
}
static DESCRIPTION: OnceLock<Description> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
pub(super) fn description() -> Result<&'static Description, PendFail> {
    DESCRIPTION.get().ok_or(PendFail::Denied)
}
fn own(source: &[Region]) -> Result<Vec<Region>, PendFail> {
    let mut v = Vec::new();
    v.try_reserve_exact(source.len())
        .map_err(|_| PendFail::OutOfMemory)?;
    v.extend_from_slice(source);
    Ok(v)
}
pub(in super::super) fn admit(d: Devices<'_>) -> Result<(), PendFail> {
    d.validate(facts::info().memory)?;
    let owned = Description {
        authorities: own(d.authorities)?,
        protected: own(d.protected)?,
        external: own(d.external)?,
        controllers: own(d.controllers)?,
    };
    DESCRIPTION.set(owned).map_err(|_| PendFail::Denied)
}
pub(in super::super) fn configured() {
    READY.store(true, Ordering::Release);
}
pub(in super::super) fn ready() -> bool {
    READY.load(Ordering::Acquire)
}
impl Description {
    pub(super) fn admits(&self, region: Region) -> Result<(), PendFail> {
        Devices {
            authorities: &self.authorities,
            protected: &self.protected,
            external: &self.external,
            controllers: &self.controllers,
            interrupt: None,
        }
        .admits_external(region)
    }
}
