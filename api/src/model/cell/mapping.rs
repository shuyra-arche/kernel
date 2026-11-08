//! 目标地址空间里一段关系的句柄（0 = 无效哨兵）。
//!
//! 与 [`SlotId`](super::SlotId) 同族，但定位还多一层约束：号只在
//! **目标地址空间那张表**里解析得动，所以每次定位都必须同时给出目标域。
//! 数字身份可跨线程传递；它本身不授予控制权。

use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;
use crate::address::VirtAddr;
use crate::model::identity::{PendId, TeamId};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct MappingId(pub(crate) usize);

impl MappingId {
    /// 无效哨兵（0）：没有这一段关系。
    pub const NONE: Self = Self(0);

    /// 号在**线上**的宽度（8 字节 LE）——与 [`SlotId`](super::SlotId)
    /// 同形：一个号在账上占多宽是该号自己的事实。
    pub const WIDTH: usize = 8;

    /// 由裸值造一枚（线上解码面）。
    pub(crate) const fn new(v: usize) -> Self {
        Self(v)
    }

    /// **收号**：号从线上/账上到来时解开它（运行时要记住一个号再交回去，
    /// 走的就是这扇"收到一枚号"的门；伪造的号在目标域那张表里查不到 ⇒ 照旧 Denied）。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let at = bytes.get(..Self::WIDTH)?;
        let raw = u64::from_le_bytes(at.try_into().ok()?);
        let raw = usize::try_from(raw).ok()?;
        (raw <= isize::MAX as usize).then(|| Self::new(raw))
    }

    /// **交出**：号变回线上的那 8 字节（与 [`MappingId::from_bytes`] 成对）。
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        (self.0 as u64).to_le_bytes()
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl From<MappingId> for usize {
    fn from(mapping: MappingId) -> Self {
        mapping.0
    }
}

impl Wire for MappingId {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.0;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        if v > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        Ok(Self::new(v))
    }
}

impl FromPair for (MappingId, VirtAddr) {
    fn from_pair(v0: usize, v1: usize) -> Self {
        (MappingId::new(v0), VirtAddr(v1))
    }
}

/// 一次 `Attach` 的控制权选择：这段映射由谁控制。
///
/// 默认 `Caller`（发起 `Attach` 的那个 Task）；`SpaceMembers` 是目标 Space 成员，
/// 只在发起者同时通过来源授权与目标域授权时才可选，且**逐映射**生效——
/// 它把这一段登记到目标 Space 自己的控制记录上，不是“同 Space 自动全控”，
/// 也不给任何来源/其它映射 加权限。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingControl {
    /// 发起 `Attach` 的那个 Task（普通私有映射的默认）。
    Caller,
    /// 目标 Space 的成员（显式选择，逐映射生效）。
    SpaceMembers,
}

impl Wire for MappingControl {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let word = match self {
            Self::Caller => 0usize,
            Self::SpaceMembers => 1usize,
        };
        word.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        match usize::unpack(s, i)? {
            0 => Ok(Self::Caller),
            1 => Ok(Self::SpaceMembers),
            _ => Err(Decode::Invalid),
        }
    }
}

/// A CPU space selected by Team control, or an I/O space selected by a trusted
/// MMIO View's Pend. Numeric addresses never mint an external routing authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingTarget {
    Cpu(TeamId),
    Io(PendId),
}

impl Wire for MappingTarget {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let raw = match *self {
            Self::Cpu(team) => {
                assert!(team.get() <= isize::MAX as usize);
                team.get()
            }
            Self::Io(token) => {
                assert!(token.get() > 0 && token.get() <= isize::MAX as usize);
                token.get() | (1usize << (usize::BITS - 1))
            }
        };
        raw.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let raw = usize::unpack(s, i)?;
        let tag = 1usize << (usize::BITS - 1);
        if raw & tag == 0 {
            Ok(Self::Cpu(TeamId::new(raw)))
        } else if raw != tag {
            Ok(Self::Io(PendId::new(raw & !tag)))
        } else {
            Err(Decode::Invalid)
        }
    }
}
