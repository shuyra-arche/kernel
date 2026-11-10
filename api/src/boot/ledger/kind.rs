//! Kernel 能力种类的 ABI 判别号。
//!
//! 启动资源目录与能力授出方共享此标签；它不定义服务消息的字节编码。

const KIND_POLE: u8 = ResourceKind::Share as u8;
const KIND_SIGN: u8 = ResourceKind::Sign as u8;
const KIND_HOLE: u8 = ResourceKind::Clone as u8;

/// **要的是哪一种门闩**（**判别号即线格式**：`repr(u8)`）。
///
/// 资源目录与授出方使用同一枚种类标签。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResourceKind {
    /// 一段内存（设备寄存器页 / 自描述区 / 载荷区）。
    Share,
    /// 空载荷的信号（中断门铃）。
    Sign = 1,
    Clone = 2,
    /// Kernel-minted execution or resource-management qualification.
    Control = 4,
    None = 5,
    /// A trusted address-space Pend, not a requester-number credential.
    Nest = 7,
}

impl ResourceKind {
    /// 从 ABI 判别号还原种类；未知值返回 `None`。
    pub const fn of(raw: u8) -> Option<ResourceKind> {
        match raw {
            KIND_POLE => Some(ResourceKind::Share),
            KIND_SIGN => Some(ResourceKind::Sign),
            KIND_HOLE => Some(ResourceKind::Clone),
            4 => Some(ResourceKind::Control),
            5 => Some(ResourceKind::None),
            // Kind 6 was Port and remains invalid.
            7 => Some(ResourceKind::Nest),
            _ => None,
        }
    }
}

impl From<crate::model::view::ViewKind> for ResourceKind {
    fn from(value: crate::model::view::ViewKind) -> Self {
        match value {
            crate::model::view::ViewKind::Clone => Self::Clone,
            crate::model::view::ViewKind::Share => Self::Share,
        }
    }
}
