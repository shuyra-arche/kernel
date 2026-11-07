//! Operation domains and aggregate Server dispatch.
pub mod chrono;
pub mod control;
pub mod debug;
pub mod device;
pub mod hole;
pub mod memory;
pub mod nole;
pub mod pie;
pub mod pole;
pub mod room;
pub mod tole;
pub mod unit;

/// Decoded environment call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvCall {
    Device(self::device::DeviceCall),
    Room(self::room::RoomCall),
    Unit(self::unit::UnitCall),
    Memory(self::memory::MemoryCall),
    Chrono(self::chrono::ChronoCall),
    Hole(self::hole::HoleCall),
    Nole(self::nole::NoleCall),
    Pole(self::pole::PoleCall),
    Tole(self::tole::ToleCall),
    Control(self::control::ControlCall),
    Pie(self::pie::PieCall),
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
            5 => Ok(EnvCall::Hole(self::hole::HoleCall::from_wire(slot, regs)?)),
            6 => Ok(EnvCall::Control(self::control::ControlCall::from_wire(
                slot, regs,
            )?)),
            7 => Ok(EnvCall::Pie(self::pie::PieCall::from_wire(slot, regs)?)),
            8 => Ok(EnvCall::Debug(self::debug::DebugCall::from_wire(
                slot, regs,
            )?)),
            10 => Ok(EnvCall::Pole(self::pole::PoleCall::from_wire(slot, regs)?)),
            11 => Ok(EnvCall::Nole(self::nole::NoleCall::from_wire(slot, regs)?)),
            12 => Ok(EnvCall::Tole(self::tole::ToleCall::from_wire(slot, regs)?)),
            13 => Ok(EnvCall::Device(self::device::DeviceCall::from_wire(
                slot, regs,
            )?)),
            _ => Err(crate::abi::register::Decode::BadSlot),
        }
    }
}
