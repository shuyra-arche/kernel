//! Kernel's retained identity-RAM access boundary. Conversion establishes only
//! mapping/attribute eligibility: callers still prove custody, lifetime,
//! exclusivity and synchronization. MMIO uses its separately admitted aperture.
use kernel_api::address::PhysAddr;
use kernel_api::boot::platform::Region;

fn checked<T>(physical: PhysAddr) -> usize {
    let address = physical.as_usize();
    assert!(address.is_multiple_of(core::mem::align_of::<T>()));
    assert!(
        crate::entry::boot::facts::info()
            .memory
            .contains_identity(Region::new(address, size_of::<T>()))
    );
    address
}

/// # Safety
/// Caller owns a live RAM object of T, with valid initialized contents, and
/// excludes reclamation and conflicting writes for the entire access.
pub(crate) unsafe fn read<T>(physical: PhysAddr) -> *const T {
    checked::<T>(physical) as *const T
}

/// # Safety
/// Caller has exclusive write custody of a live RAM object of T and satisfies
/// its synchronization/retirement rules. This conversion grants no authority.
pub(crate) unsafe fn write<T>(physical: PhysAddr) -> *mut T {
    let address = checked::<T>(physical);
    unsafe extern "C" {
        static _kernel_base: u8;
        static _text_end: u8;
        static _rodata_start: u8;
    }
    let end = address
        .checked_add(size_of::<T>())
        .expect("physical object overflow");
    assert!(
        end <= (&raw const _kernel_base).addr() || address >= (&raw const _text_end).addr(),
        "write to Kernel text alias"
    );
    assert!(
        end <= (&raw const _rodata_start).addr() || address >= crate::metal::image::kernel_edge(),
        "write to Kernel ROData alias"
    );
    address as *mut T
}
