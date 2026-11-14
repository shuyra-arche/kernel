//! Publication and logical-source admission failures. These are internal
//! resource errors, independent of the retired Cell register-call domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessError {
    /// 句柄不属于调用者、访问位非法，或来源/目标控制未获授权。
    Denied,
    /// 段耗尽 / 物理帧耗尽 / 映射表备不下。
    OutOfMemory,
    /// 地址、范围坐标或选址描述不满足格式与对齐约束。
    NotAligned,
    /// 这一段不在任何已声明的映射里。
    NoRegion,
    /// 该 VA 已被映射。
    AlreadyMapped,
    /// 借入页（别人的所有权）不许加宽。
    WidenDenied,
    Busy,
    /// 移交的源页空着：没有可交出去的承载。
    SourceEmpty,
}

use crate::basis::{flat::FlatError, nest::NestError};
impl From<NestError> for AccessError {
    fn from(error: NestError) -> Self {
        match error {
            NestError::OutOfMemory => AccessError::OutOfMemory,
            NestError::NotAligned => AccessError::NotAligned,
            NestError::NoRegion | NestError::Missing => AccessError::NoRegion,
            NestError::AlreadyMapped => AccessError::AlreadyMapped,
            NestError::WidenDenied => AccessError::WidenDenied,
            NestError::Shootdown | NestError::Busy => AccessError::Busy,
            NestError::Pinned | NestError::NotMapped | NestError::DramOverlap => {
                AccessError::Denied
            }
        }
    }
}

impl From<FlatError> for AccessError {
    fn from(error: FlatError) -> Self {
        NestError::from(error).into()
    }
}

impl From<crate::basis::cell::page::RetainError> for AccessError {
    fn from(error: crate::basis::cell::page::RetainError) -> Self {
        use crate::basis::cell::page::RetainError;
        match error {
            RetainError::Exhausted => Self::OutOfMemory,
            RetainError::Quarantined => Self::Denied,
            RetainError::Busy | RetainError::Borrowed => Self::Busy,
            RetainError::Unbalanced => unreachable!("unbalanced Page change"),
        }
    }
}
