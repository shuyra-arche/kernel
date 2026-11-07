pub(crate) mod identity;
pub(crate) mod mode;
pub(crate) mod synchronize;
pub(crate) mod table;

use kernel_api::address::PAGE_SHIFT;
use kernel_api::address::VirtAddr;

/// Canonicalize an internal address using the active translation geometry.
pub(crate) fn canonical(addr: usize) -> VirtAddr {
    let bit = mode::geometry(mode::mode()).split_bit() as usize;
    VirtAddr::new((((addr as isize) << (63 - bit)) >> (63 - bit)) as usize)
}

pub(crate) trait Address {
    fn vpn(self, level: u8) -> usize;
    fn is_user(self) -> bool;
    fn is_kernel(self) -> bool;
}
impl Address for VirtAddr {
    fn vpn(self, level: u8) -> usize {
        (self.get() >> (PAGE_SHIFT + level as usize * 9)) & 0x1ff
    }
    fn is_user(self) -> bool {
        (self.get() >> mode::geometry(mode::mode()).split_bit()) & 1 == 0
    }
    fn is_kernel(self) -> bool {
        if !self.is_user() {
            return true;
        }
        unsafe extern "C" {
            static _kernel_base: u8;
            static _kernel_edge: u8;
        }
        self.get() >= (&raw const _kernel_base).addr()
            && self.get() < (&raw const _kernel_edge).addr()
    }
}
