use super::super::super::BatchId;
use super::super::super::Error;
use super::super::super::HardwareError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Command([u64; 2]);

impl Command {
    /// Host 第一阶段全范围失效，包含全局翻译。不适用于第二阶段或设备 ATC。
    pub(crate) fn invalidate_host() -> Self {
        Self([1, 0])
    }

    pub(crate) fn invalidate_directory() -> Self {
        Self([3, 0])
    }

    pub(crate) fn fence(
        batch: BatchId,
        completion_pa: u64,
        modifiable: bool,
    ) -> Result<Self, Error> {
        if completion_pa == 0 || !completion_pa.is_multiple_of(4) {
            return Err(Error::InvalidRange);
        }
        let modifiability = (1 << 12) | if modifiable { 1 << 13 } else { 0 };
        Ok(Self([
            2 | (1 << 10) | modifiability | (u64::from(batch.value()) << 32),
            completion_pa >> 2,
        ]))
    }

    pub(crate) fn interrupt(mut self) -> Self {
        self.0[0] |= 1 << 11;
        self
    }

    pub(crate) fn words(self) -> [u64; 2] {
        self.0
    }
}

/// 只证明 CQ fence 的完成值匹配；不等同于 Hardware::observe 的完整安全证明。
pub(crate) fn fence_complete(
    batch: BatchId,
    cqcsr: u32,
    completion: u32,
) -> Result<bool, HardwareError> {
    if cqcsr & (1 << 9) != 0 {
        return Err(HardwareError::Timeout);
    }
    if cqcsr & ((1 << 8) | (1 << 10)) != 0 {
        return Err(HardwareError::Fault);
    }
    if cqcsr & (1 << 16) == 0 || cqcsr & (1 << 17) != 0 {
        return Ok(false);
    }
    Ok(completion == batch.value())
}
