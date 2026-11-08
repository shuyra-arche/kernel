pub(crate) mod fence;
pub(crate) mod riscv;
mod table;

pub(crate) use riscv::mode::{Geo as Geometry, lower, upper};
pub(crate) use riscv::table::TableNode as Table;
pub(crate) use riscv::{canonical, identity};
pub(crate) use table::{Attributes, PageTable, Root};

pub(crate) fn geometry() -> Geometry {
    riscv::mode::geometry(riscv::mode::mode())
}
