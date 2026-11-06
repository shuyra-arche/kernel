//! Kernel 能力种类的 ABI 判别号。
//!
//! 启动资源目录与能力授出方共享此标签；它不定义服务消息的字节编码。

const KIND_POLE: u8 = PieKind::Pole as u8;
const KIND_NOLE: u8 = PieKind::Nole as u8;
const KIND_HOLE: u8 = PieKind::Hole as u8;
const KIND_TOLE: u8 = PieKind::Tole as u8;

/// **要的是哪一种门闩**（**判别号即线格式**：`repr(u8)`）。
///
/// 资源目录与授出方使用同一枚种类标签。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieKind {
    /// 一段内存（设备寄存器页 / 自描述区 / 载荷区）。
    Pole,
    /// 空载荷的信号（中断门铃）。
    Nole,
    Hole,
    Tole,
}

impl PieKind {
    /// 从 ABI 判别号还原种类；未知值返回 `None`。
    pub const fn of(raw: u8) -> Option<PieKind> {
        match raw {
            KIND_POLE => Some(PieKind::Pole),
            KIND_NOLE => Some(PieKind::Nole),
            KIND_HOLE => Some(PieKind::Hole),
            KIND_TOLE => Some(PieKind::Tole),
            _ => None,
        }
    }
}
