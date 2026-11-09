pub(crate) mod domain;
pub(crate) mod identity;
pub(crate) mod mode;
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
}
impl Address for VirtAddr {
    fn vpn(self, level: u8) -> usize {
        (self.get() >> (PAGE_SHIFT + level as usize * 9)) & 0x1ff
    }
    fn is_user(self) -> bool {
        (self.get() >> mode::geometry(mode::mode()).split_bit()) & 1 == 0
    }
}

pub(crate) fn encode_root(root: crate::metal::mmu::Root) -> ::riscv::register::satp::Satp {
    ::riscv::register::satp::Satp::from_bits(
        (mode::mode().into_usize() << 60)
            | (root.asid.get() << 44)
            | (root.frame.as_usize() >> PAGE_SHIFT),
    )
}

/// # Safety
/// The root maps the currently executing Kernel and the Hart's active stack.
pub(crate) unsafe fn activate(root: crate::metal::mmu::Root) {
    let asid = root.asid;
    unsafe {
        ::riscv::register::satp::set(
            mode::mode(),
            asid.get(),
            root.frame.as_usize() >> PAGE_SHIFT,
        );
        identity::flush(asid);
    }
}

pub(crate) use domain::Domain;
pub(crate) use mode::{Geo as Geometry, lower, upper};
pub(crate) use table::TableNode as Table;
pub(crate) type Root = crate::metal::generic::mmu::Root<identity::Asid>;
