//! Checked six-word request encodings.

pub trait Wire: Sized {
    /// Exact register count consumed by both pack and unpack.
    const WORDS: usize;
    /// 把自身 pack 进 `s`，游标 `i` 前进 `WORDS` 格。
    fn pack(&self, s: &mut [usize; 6], i: &mut usize);
    /// 从 `s` 读出自身，游标 `i` 前进 `WORDS` 格；非法位 → `Err(Decode)`。
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode>;
}

/// 解码错误：unpack 失败（含非法位校验拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decode {
    /// 未知调用号（index 越界）。
    BadSlot,
    /// 字段数超出 a0..a5。
    Overflow,
    /// 非法位（如 `Permission` 含未定义位，或 bool 非 0/1）。
    Invalid,
}

// ── 基元 ────────────────────────────────────────────────────────────────

impl Wire for usize {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        Ok(v)
    }
}

impl Wire for u64 {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)? as u64;
        *i += 1;
        Ok(v)
    }
}

impl Wire for crate::model::wait::Wait {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.to_wire();
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        // 满射（每个 `usize` 都有一格）⇒ 解码这一步没有非法位可拒。
        Ok(crate::model::wait::Wait::from_wire(v))
    }
}

impl Wire for bool {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Decode::Invalid),
        }
    }
}

impl Wire for crate::model::unit::ProgramKind {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = match self {
            crate::model::unit::ProgramKind::User => 0,
            crate::model::unit::ProgramKind::Supervisor => 1,
        };
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        match v {
            0 => Ok(crate::model::unit::ProgramKind::User),
            1 => Ok(crate::model::unit::ProgramKind::Supervisor),
            _ => Err(Decode::Invalid),
        }
    }
}

/// 权限位掩码（envcall 单一真相；unpack 走 `from_bits` 校验，非法位 → `Invalid`，
/// 替代旧 `from_bits_truncate` 的静默截断）。
///
/// **超宽值同样要拒**：`a2`/`a3` 是整寄存器（`usize`），故「先 `as u32` 再校验」等于
/// 把 32 位以上静默丢掉后再判合法——`0x1_0000_0002` 会被解成 `STORE` 而不是 `Invalid`，
/// 那正是本条要根除的那类静默截断，只是搬到了高位。故先判宽度、再判位。
impl Wire for crate::model::permission::Permission {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.bits() as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        let bits = u32::try_from(v).map_err(|_| Decode::Invalid)?;
        crate::model::permission::Permission::from_bits(bits).ok_or(Decode::Invalid)
    }
}

mod hole;
mod identity;
mod memory;
mod pie;
mod pole;
mod tole;
mod unit;
