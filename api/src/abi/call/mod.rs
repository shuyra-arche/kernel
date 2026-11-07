//! Operation domains and aggregate Server dispatch.
pub mod chrono;
pub mod clone_view;
pub mod control;
pub mod debug;
pub mod memory;
pub mod pend;
pub mod room;
pub mod share_view;
pub mod sign;
pub mod unit;

/// Decoded environment call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvCall {
    Room(self::room::RoomCall),
    Unit(self::unit::UnitCall),
    Memory(self::memory::MemoryCall),
    Chrono(self::chrono::ChronoCall),
    Clone(self::clone_view::CloneCall),
    Share(self::share_view::ShareCall),
    Sign(self::sign::SignCall),
    Control(self::control::ControlCall),
    Pend(self::pend::PendCall),
    /// 调试面（见 [`self::debug::DebugCall`]）。
    Debug(self::debug::DebugCall),
}

impl EnvCall {
    /// 由调用号 + 寄存器组解码回带载荷的聚合枚举。
    pub fn from_wire(slot: usize, regs: &[usize; 6]) -> Result<Self, crate::abi::register::Decode> {
        let class = slot >> 32;
        match class {
            0 => Ok(EnvCall::Room(self::room::RoomCall::from_wire(slot, regs)?)),
            1 => Ok(EnvCall::Unit(self::unit::UnitCall::from_wire(slot, regs)?)),
            2 => Ok(EnvCall::Memory(self::memory::MemoryCall::from_wire(
                slot, regs,
            )?)),
            4 => Ok(EnvCall::Chrono(self::chrono::ChronoCall::from_wire(
                slot, regs,
            )?)),
            5 => Ok(EnvCall::Clone(self::clone_view::CloneCall::from_wire(
                slot, regs,
            )?)),
            6 => Ok(EnvCall::Control(self::control::ControlCall::from_wire(
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
