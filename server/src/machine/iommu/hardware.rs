use super::BatchId;
use super::HardwareError;
use super::Projection;
use super::riscv::Fault;

/// 只能由 Kernel 安装的单一设备保护域适配器，不接受 Unit 提供的完成回调。
/// 每个方法是有界进度步骤；错误允许已经产生部分硬件效果。
///
/// # Safety
/// 实现者须独占或可信仲裁该域的上下文、映射、命令与完成存储，阻止旁路重授权。
/// publish 不得公开 Projection 之外的承载，ready 确认投影已可用。revoke 必须幂等，
/// 登记撤销意图；fence 关联唯一批次。observe 推进撤销与后续同步，不能越过其中任何一步。
/// observe 只能报告真实完成批次，并保证对应投影已经不可达、先前访问与平台同步
/// 全部结清；单个 CQ 完成字不足以无条件满足此契约。错误/超时不能转换成完成。
/// 完成存储与队列不可由外部 Unit 写入，迟到写回不能污染复用后的存储。
/// 实现者的 Drop 须安全处理自己的目录、页表和队列等硬件可访问内存。
pub(crate) unsafe trait Hardware {
    fn capacity(&self) -> usize {
        usize::MAX
    }
    /// Validate before retaining or publishing; must not change hardware state.
    fn validate(&self, _projection: Projection) -> Result<(), HardwareError> {
        Ok(())
    }
    fn publish(&mut self, projection: Projection) -> Result<(), HardwareError>;
    fn ready(&mut self, _projection: Projection) -> Result<bool, HardwareError> {
        Ok(true)
    }
    fn revoke(&mut self, projection: Projection) -> Result<(), HardwareError>;
    fn fence(&mut self, projection: Projection, batch: BatchId) -> Result<(), HardwareError>;
    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError>;
    fn fault(&mut self) -> Result<Option<Fault>, HardwareError> {
        Ok(None)
    }
    /// Called only after all payload obligations are gone. Includes cancelling
    /// unpublished setup: queue/table storage must also be safely discharged.
    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        Ok(true)
    }
}

// SAFETY: exclusive forwarding preserves the installed implementation's contract.
unsafe impl<H: Hardware + ?Sized> Hardware for alloc::boxed::Box<H> {
    fn capacity(&self) -> usize {
        (**self).capacity()
    }
    fn validate(&self, p: Projection) -> Result<(), HardwareError> {
        (**self).validate(p)
    }
    fn publish(&mut self, p: Projection) -> Result<(), HardwareError> {
        (**self).publish(p)
    }
    fn ready(&mut self, p: Projection) -> Result<bool, HardwareError> {
        (**self).ready(p)
    }
    fn revoke(&mut self, p: Projection) -> Result<(), HardwareError> {
        (**self).revoke(p)
    }
    fn fence(&mut self, p: Projection, b: BatchId) -> Result<(), HardwareError> {
        (**self).fence(p, b)
    }
    fn observe(&mut self, b: BatchId) -> Result<Option<BatchId>, HardwareError> {
        (**self).observe(b)
    }
    fn fault(&mut self) -> Result<Option<Fault>, HardwareError> {
        (**self).fault()
    }
    fn shutdown(&mut self) -> Result<bool, HardwareError> {
        (**self).shutdown()
    }
}
