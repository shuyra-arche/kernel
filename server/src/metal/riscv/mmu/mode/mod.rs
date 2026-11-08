use riscv::register::satp;

use crate::metal::board::{self, harts::Paging};
use crate::metal::sync::OnceLock;
use kernel_api::address::VirtAddr;

mod error;
mod geometry;

pub use error::SatpError;
pub use geometry::Geo;
pub use geometry::geometry;

static MODE: OnceLock<satp::Mode> = OnceLock::new();

/// Policy: use the largest mode common to every admitted hart. Sv57 implies
/// Sv48, which implies Sv39. No temporary page table or satp probe is needed.
pub fn init() -> Result<(), SatpError> {
    let mode = match board::info().hart.common.paging {
        Paging::Sv39 => satp::Mode::Sv39,
        Paging::Sv48 => satp::Mode::Sv48,
        Paging::Sv57 => satp::Mode::Sv57,
    };
    MODE.set(mode).map_err(|_| SatpError::AlreadyInit)
}

pub fn mode() -> satp::Mode {
    *MODE.get().expect("page table mode not initialized")
}

pub fn levels() -> usize {
    geometry(mode()).levels as usize
}

pub fn lower() -> VirtAddr {
    crate::metal::riscv::mmu::canonical(1usize << geometry(mode()).split_bit())
}

pub fn upper() -> VirtAddr {
    VirtAddr::wrap(1usize << geometry(mode()).split_bit())
}
