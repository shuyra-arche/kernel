//! Once-only admitted boot facts. The integration owner supplies normalized
//! descriptors; Kernel owns publication and validates its executing image.
use super::BootError;
use crate::metal::{hart, sync::OnceLock};
use kernel_api::boot::platform::{Platform, Region};
static PLATFORM: OnceLock<Platform> = OnceLock::new();
pub(super) fn init(platform: Platform) -> Result<(), BootError> {
    if !platform.memory.protects(super::image_region())
        || platform.hart.firmware(hart::HartId::new(0)) != hart::firmware_id()
    {
        return Err(BootError::Description("boot image or boot hart mismatch"));
    }
    PLATFORM
        .set(platform)
        .map_err(|_| BootError::Description("boot facts already admitted"))
}
pub(crate) fn info() -> &'static Platform {
    PLATFORM.get().expect("boot facts not admitted")
}
pub(crate) fn dram_edge() -> Option<usize> {
    PLATFORM.get().map(|p| p.memory.envelope().range().end)
}
pub(crate) fn contains_identity(base: usize, size: usize) -> bool {
    PLATFORM
        .get()
        .is_some_and(|p| p.memory.contains_identity(Region::new(base, size)))
}
