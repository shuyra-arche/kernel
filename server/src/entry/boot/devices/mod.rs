mod admission;
pub(super) mod external;
mod iommu;
pub(super) use admission::{admit, configured, ready};

pub(super) use external::init as init_external;
pub use external::{ExternalRoute, MapMmio};
pub(super) use iommu::{init as init_iommu, shutdown as shutdown_iommu};

use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::SourceAuthorization;
use crate::basis::view::View;
use crate::entry::boot::facts;
use crate::entry::boot::resources::Registry;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::Page;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::TaskId;

pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    let dtb = facts::info().dtb();
    for region in admission::description()?.authorities.iter() {
        grant_region(registry, *region)?;
    }
    for region in external::REGIONS.lock().iter() {
        grant_region(registry, *region)?;
    }
    for port in crate::basis::port::PORTS.lock().iter() {
        let root = pend::PendEntry::try_new(
            pend::Target::Port(port.clone()),
            Mark::NONE,
            Modifiable::No,
            None,
        )?;
        registry
            .register(Name::Port(port.id().get() as u64), root)
            .map_err(|e| e.into_parts().0)?;
    }
    let meta = crate::basis::flat::sharing::region(dtb.base, dtb.size, TaskId::new(0))?;
    let root = pend::PendEntry::try_new(View::Share(meta), Mark::NONE, Modifiable::No, None)?;
    registry
        .register(Name::Page(Page::Dtb), root)
        .map_err(|e| e.into_parts().0)?;
    if let Some(initrd) = facts::info().initrd() {
        let meta = crate::basis::flat::sharing::region(initrd.base, initrd.size, TaskId::new(0))?;
        let root = pend::PendEntry::try_new(View::Share(meta), Mark::NONE, Modifiable::No, None)?;
        registry
            .register(Name::Page(Page::Initrd), root)
            .map_err(|e| e.into_parts().0)?;
    }
    Ok(())
}

fn grant_region(
    registry: &mut Registry,
    region: kernel_api::boot::platform::Region,
) -> Result<(), PendFail> {
    let (base, size) = (region.base, region.size);
    let meta = crate::basis::flat::sharing::region(base, size, TaskId::new(0))?;
    let source = meta.source();
    let root =
        pend::PendEntry::try_new(View::Share(meta.clone()), Mark::NONE, Modifiable::Yes, None)?;
    // 借来的区也算一份来源：把授权记录接上，持有者就能取得**自己那一小段**子窗口
    // （而不是只能攞整段）。与 `Unseal::Share` 接手私有根同一套，不另造旁路。
    // 上限就是 MMIO 的读写两位：设备区不开执行映射。
    let authorization =
        SourceAuthorization::new(source, Access::READ | Access::WRITE, TaskId::new(0))?;
    meta.attach_source(&authorization);
    authorization.adopt(&meta, root.token(), root.authority().clone())?;
    registry
        .register(Name::Page(Page::Region(base as u64)), root)
        .map_err(|e| e.into_parts().0)
}
