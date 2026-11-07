mod plan;
mod ranges;
mod region;
mod reserved;

use crate::core::memory::nest::Access;
use crate::core::work::mail::{self, Mail};
use crate::core::work::pend::{self, Permission, SourceAuthorization};
use crate::entry::boot::resources::Registry;
use crate::machine::board;
use kernel_api::{
    abi::call::pie::PieFail, boot::ledger::Name, boot::ledger::Page, model::identity::Mark,
    model::identity::TaskId,
};
use plan::Plan;

pub(crate) fn register(registry: &mut Registry) -> Result<(), PieFail> {
    let dtb = board::info().dtb();
    let fdt = board::device_tree().ok_or(PieFail::Denied)?;
    let dram = board::info().dram;
    // Discover and validate every reservation before creating any device authority.
    let plan = Plan::discover(&fdt, dram.base, dram.size)?;
    for region in plan.grants() {
        grant_region(registry, *region)?;
    }
    let meta = mail::pole::region(dtb.base, dtb.size, TaskId::new(0))?;
    let root = pend::Pie::try_new(
        Mail::Pole(meta),
        Mark::NONE,
        Permission::FETCH | Permission::VEST,
        None,
    )?;
    registry
        .register(Name::Page(Page::Dtb), root)
        .map_err(|e| e.into_parts().0)?;
    if let Some(initrd) = board::info().initrd() {
        let meta = mail::pole::region(initrd.base, initrd.size, TaskId::new(0))?;
        let root = pend::Pie::try_new(
            Mail::Pole(meta),
            Mark::NONE,
            Permission::FETCH | Permission::VEST,
            None,
        )?;
        registry
            .register(Name::Page(Page::Initrd), root)
            .map_err(|e| e.into_parts().0)?;
    }
    Ok(())
}

fn grant_region(registry: &mut Registry, region: region::Region) -> Result<(), PieFail> {
    let (base, size) = (region.base, region.size);
    let meta = mail::pole::region(base, size, TaskId::new(0))?;
    let source = meta.source();
    let root = pend::Pie::try_new(
        Mail::Pole(meta.clone()),
        Mark::NONE,
        Permission::FETCH | Permission::STORE | Permission::VEST | Permission::ONLY,
        None,
    )?;
    // 借来的区也算一份来源：把授权记录接上，持有者就能取得**自己那一小段**子窗口
    // （而不是只能攞整段）。与 `Unseal::Pole` 接手私有根同一套，不另造旁路。
    // 上限就是 MMIO 的读写两位：设备区不开执行映射。
    let authorization =
        SourceAuthorization::new(source, Access::READ | Access::WRITE, TaskId::new(0))?;
    meta.attach_source(&authorization);
    authorization.adopt(&meta, root.token(), root.grant().clone())?;
    registry
        .register(Name::Page(Page::Region(base as u64)), root)
        .map_err(|e| e.into_parts().0)
}
