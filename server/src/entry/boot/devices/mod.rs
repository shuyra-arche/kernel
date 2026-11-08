mod iommu;
mod pci;
mod plan;

use plan::reserved::discover as iommu_regions;

pub(super) use iommu::{init as init_iommu, shutdown as shutdown_iommu};
pub use pci::PciRoute;
pub(super) use pci::init as init_pci;

use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::SourceAuthorization;
use crate::basis::view;
use crate::basis::view::View;
use crate::entry::boot::resources::Registry;
use crate::machine::board;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::boot::ledger::Name;
use kernel_api::boot::ledger::Page;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::TaskId;
use plan::Plan;

pub(crate) fn register(registry: &mut Registry) -> Result<(), PendFail> {
    let dtb = board::info().dtb();
    let fdt = board::device_tree().ok_or(PendFail::Denied)?;
    let dram = board::info().memory.envelope();
    // Discover and validate every reservation before creating any device authority.
    let plan = Plan::discover(&fdt, dram.base, dram.size)?;
    for region in plan.authorities() {
        grant_region(registry, *region)?;
    }
    // Export each validated BAR through the ordinary View/Pend boot ledger.
    let apertures = crate::machine::iommu::riscv::pci::aperture::APERTURES.lock();
    for aperture in apertures.iter() {
        let source = aperture.source.clone();
        let size = source
            .page_count()
            .checked_bytes()
            .ok_or(PendFail::Denied)?;
        let meta = view::ShareView::from_source(source.clone(), size, TaskId::new(0))?;
        let root =
            pend::PendEntry::try_new(View::Share(meta.clone()), Mark::NONE, Modifiable::Yes, None)?;
        let authorization =
            SourceAuthorization::new(source, Access::READ | Access::WRITE, TaskId::new(0))?;
        meta.attach_source(&authorization);
        authorization.adopt(&meta, root.token(), root.authority().clone())?;
        authorization.consume();
        meta.commit_source();
        registry
            .register(Name::Page(Page::Region(aperture.port.base() as u64)), root)
            .map_err(|e| e.into_parts().0)?;
    }
    drop(apertures);
    let meta = view::share::region(dtb.base, dtb.size, TaskId::new(0))?;
    let root = pend::PendEntry::try_new(View::Share(meta), Mark::NONE, Modifiable::No, None)?;
    registry
        .register(Name::Page(Page::Dtb), root)
        .map_err(|e| e.into_parts().0)?;
    if let Some(initrd) = board::info().initrd() {
        let meta = view::share::region(initrd.base, initrd.size, TaskId::new(0))?;
        let root = pend::PendEntry::try_new(View::Share(meta), Mark::NONE, Modifiable::No, None)?;
        registry
            .register(Name::Page(Page::Initrd), root)
            .map_err(|e| e.into_parts().0)?;
    }
    Ok(())
}

fn grant_region(
    registry: &mut Registry,
    region: board::tree::mmio::Region,
) -> Result<(), PendFail> {
    let (base, size) = (region.base, region.size);
    let meta = view::share::region(base, size, TaskId::new(0))?;
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

/// Validate the exact same authority plan before configuring any protected endpoint.
pub(super) fn validate(fdt: &fdt::Fdt) -> Result<(), PendFail> {
    let dram = board::info().memory.envelope();
    Plan::discover(fdt, dram.base, dram.size).map(|_| ())
}
