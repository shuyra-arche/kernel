pub(crate) mod riscv;

pub(crate) use riscv::mode::{Geo as Geometry, lower, upper};
pub(crate) use riscv::table::TableNode as Table;
pub(crate) use riscv::{canonical, identity};
pub(crate) type Root = crate::metal::generic::mmu::Root<identity::Asid>;

pub(crate) fn geometry() -> Geometry {
    riscv::mode::geometry(riscv::mode::mode())
}
