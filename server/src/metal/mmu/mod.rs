pub(crate) mod fence;
use super::riscv::mmu as backend;
mod table;

pub(crate) use backend::mode::{Geo as Geometry, lower, upper};
pub(crate) use backend::table::TableNode as Table;
pub(crate) use backend::{canonical, identity};
pub(crate) use table::{Attributes, PageTable, Root};

pub(crate) fn geometry() -> Geometry {
    backend::mode::geometry(backend::mode::mode())
}
