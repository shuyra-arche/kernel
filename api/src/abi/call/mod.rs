//! Operation domains and aggregate Server dispatch.
pub mod cell;
pub mod debug;
pub mod pend;
pub mod room;
pub mod sign;
pub mod unit;
mod view;

pub use view::{clone as clone_view, share as share_view};

/// Decoded environment call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvCall {
    Room(self::room::RoomCall),
    Unit(self::unit::UnitCall),
    Cell(self::cell::CellCall),
    Clone(self::clone_view::CloneCall),
    Share(self::share_view::ShareCall),
    Sign(self::sign::SignCall),
    Pend(self::pend::PendCall),
    /// 调试面（见 [`self::debug::DebugCall`]）。
    Debug(self::debug::DebugCall),
}

impl EnvCall {
    /// 由调用号 + 寄存器组解码回带载荷的聚合枚举。
    pub fn from_wire(slot: usize, regs: &[usize; 6]) -> Result<Self, crate::abi::register::Decode> {
        if (slot >> 48) != crate::abi::VERSION as usize {
            return Err(crate::abi::register::Decode::BadSlot);
        }
        let class = (slot >> 32) & 0xffff;
        match class {
            0 => Ok(EnvCall::Room(self::room::RoomCall::from_wire(slot, regs)?)),
            1 => Ok(EnvCall::Unit(self::unit::UnitCall::from_wire(slot, regs)?)),
            2 => Ok(EnvCall::Cell(self::cell::CellCall::from_wire(slot, regs)?)),
            5 => Ok(EnvCall::Clone(self::clone_view::CloneCall::from_wire(
                slot, regs,
            )?)),
            7 => Ok(EnvCall::Pend(self::pend::PendCall::from_wire(slot, regs)?)),
            8 => Ok(EnvCall::Debug(self::debug::DebugCall::from_wire(
                slot, regs,
            )?)),
            10 => Ok(EnvCall::Share(self::share_view::ShareCall::from_wire(
                slot, regs,
            )?)),
            11 => Ok(EnvCall::Sign(self::sign::SignCall::from_wire(slot, regs)?)),
            _ => Err(crate::abi::register::Decode::BadSlot),
        }
    }
}
