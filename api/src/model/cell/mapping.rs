//! Descriptive mapping identity used by completion observations. Ordinary Cell
//! operations require a live mapping-scoped Nest Pend, never this numeric ID alone.

use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;
use crate::address::VirtAddr;

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
