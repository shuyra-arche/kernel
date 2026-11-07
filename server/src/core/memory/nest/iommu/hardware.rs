use super::{BatchId, HardwareError, Projection};

/// 只能由 Kernel 安装的单一设备保护域适配器，不接受 Unit 提供的完成回调。
/// 每个方法是有界进度步骤；错误允许已经产生部分硬件效果。
///
/// # Safety
/// 实现者须独占或可信仲裁该域的上下文、映射、命令与完成存储，阻止旁路重授权。
/// publish 不得公开 Projection 之外的承载。revoke 必须幂等，撤销该关系并提交必要
/// 的 IOMMU/目录/设备缓存失效；fence 必须在那些动作之后提交与 batch 唯一关联的同步。
/// observe 只能报告真实完成批次，并保证对应投影已经不可达、先前访问与平台同步
/// 全部结清；单个 CQ 完成字不足以无条件满足此契约。错误/超时不能转换成完成。
/// 完成存储与队列不可由外部 Unit 写入，迟到写回不能污染复用后的存储。
/// 实现者的 Drop 须安全处理自己的目录、页表和队列等硬件可访问内存。
pub(crate) unsafe trait Hardware {
    fn publish(&mut self, projection: Projection) -> Result<(), HardwareError>;
    fn revoke(&mut self, projection: Projection) -> Result<(), HardwareError>;
    fn fence(&mut self, projection: Projection, batch: BatchId) -> Result<(), HardwareError>;
    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError>;
}
