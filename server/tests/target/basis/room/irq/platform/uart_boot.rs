//! Test-only ABI13 QEMU UART admission; never enable this for production.
//! Source: Vanllia f2dfa14f5b1f7bd47ee84841ba5c7112e8475950, with UART authority transformation.
//! Trusted firmware entry. Early parsing is bounded and allocation-free.
use kernel::entry::boot::{self, BootError};
use kernel_api::boot::platform::{
    Memory,
    riscv::{ClockAccess, FirmwareId, HartId, Harts, Platform},
};
use spin::Once;
static MEMORY: Once<Memory> = Once::new();
static HARTS: Once<crate::harts::HartInfo> = Once::new();
static ADMITTED: Once<Harts> = Once::new();
fn memory(dtp: usize) -> Result<&'static Memory, BootError> {
    MEMORY.try_call_once(|| {
        let fdt = unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }
            .map_err(|_| BootError::Description("invalid FDT header"))?;
        let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, fdt.total_size()) };
        crate::memory::parse(bytes, dtp, boot::image_region())
            .map_err(|_| BootError::Description("invalid memory description"))
    })
}
/// # Safety
/// Firmware supplies a readable immutable DTB and exclusively owned memory.
/// This image is paired with the board's trusted hardware/coherence contract.
pub unsafe fn init(hart: usize, dtp: usize) -> Result<(), BootError> {
    let memory = memory(dtp)?;
    let bytes = unsafe { core::slice::from_raw_parts(dtp as *const u8, memory.metadata().size) };
    let fdt = fdt::Fdt::new(bytes).map_err(|_| BootError::Description("invalid FDT"))?;
    let harts = HARTS.try_call_once(|| {
        crate::harts::HartInfo::parse(&fdt, FirmwareId::new(hart))
            .map_err(|_| BootError::Description("invalid CPU description"))
    })?;
    let admitted = ADMITTED.try_call_once(|| {
        harts
            .normalize()
            .map_err(|_| BootError::Description("invalid CPU admission"))
    })?;
    let allows = |name| {
        fdt.find_node("/chosen")
            .is_some_and(|n| matches!(crate::tree::property(n, name), Ok(Some([]))))
    };
    let clock = ClockAccess {
        user_time: allows("shuyra,user-time"),
        supervisor_stimecmp: allows("shuyra,supervisor-stimecmp"),
    };
    unsafe {
        boot::init(Platform {
            hart: admitted,
            memory,
            clock,
        })?;
    }
    let devices = crate::resources::Devices::discover(&fdt, memory, harts, HartId::new(0))
        .map_err(|source| BootError::Resources {
            operation: boot::ResourceOperation::Devices,
            source,
        })?;
    // This private QEMU image exposes UART only to its initial Team.
    // Production Platform continues to protect that resource.
    let mut description = devices.description();
    let mut authorities = alloc::vec::Vec::from(description.resources.authorities);
    let at = authorities
        .iter()
        .position(|r| r.base == 0x1000_0000)
        .ok_or(BootError::Description("missing exclusive QEMU UART"))?;
    let uart = authorities.remove(at);
    let mut external = alloc::vec::Vec::from(description.resources.external);
    external.push(uart);
    description.resources.authorities = &authorities;
    description.resources.external = &external;
    unsafe {
        boot::configure(description)?;
    }
    for (controller, source, region) in devices.driver_interrupts() {
        unsafe {
            boot::grant_interrupt(controller, source, region)?;
        }
    }
    Ok(())
}
