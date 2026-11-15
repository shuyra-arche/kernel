//! Hardware contracts for external translations, requester admission and retirement.

use alloc::boxed::Box;
use core::num::{NonZeroU32, NonZeroUsize};

/// Stable identity of an external binding, unique across I/O spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BindingId(NonZeroUsize);

impl BindingId {
    pub(crate) fn epoch(self) -> core::num::NonZeroU64 {
        core::num::NonZeroU64::new(self.0.get() as u64).unwrap()
    }
    /// # Safety
    /// The value was issued by the boot-wide allocator and has never been used.
    pub(crate) unsafe fn from_unique(value: NonZeroUsize) -> Self {
        Self(value)
    }
}

/// Stable retirement batch identity. Backends may encode its 32-bit value in
/// their completion records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BatchId(NonZeroU32);

impl BatchId {
    /// # Safety
    /// The value was issued by the boot-wide allocator and has never been used.
    pub(crate) unsafe fn from_unique(value: NonZeroU32) -> Self {
        Self(value)
    }

    pub(crate) fn value(self) -> u32 {
        self.0.get()
    }
}

/// I/O address: no CPU sign extension, user window, execution or A/D semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IoAddress(usize);
impl IoAddress {
    pub(crate) fn page(raw: usize) -> Option<Self> {
        (raw.is_multiple_of(kernel_api::address::PAGE_SIZE)
            && raw.checked_add(kernel_api::address::PAGE_SIZE).is_some())
        .then_some(Self(raw))
    }
    pub(crate) fn get(self) -> usize {
        self.0
    }
}

/// 一整个托管 Page 的设备地址投影；身份不从 IOVA 或 PA 推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Projection<P> {
    pub(crate) id: BindingId,
    pub(crate) cell: P,
    pub(crate) iova: IoAddress,
    pub(crate) physical: usize,
    pub(crate) size: usize,
    pub(crate) modifiable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareError {
    Unavailable,
    Fault,
    Timeout,
}

/// Scope of the independently proved platform boundary. A range retirement
/// leaves the rest of the context live; Binding permits replacing the context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetirementScope {
    Range { address: usize, size: usize },
    Binding,
}

/// Kernel-issued, non-reusable retirement attempt. Returning a token from a
/// different controller, requester, scope, publication or retry is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retirement {
    controller: usize,
    requester: u32,
    binding: BindingId,
    batch: BatchId,
    scope: RetirementScope,
}

impl Retirement {
    pub(crate) fn new(
        controller: usize,
        requester: u32,
        binding: BindingId,
        batch: BatchId,
    ) -> Self {
        Self {
            controller,
            requester,
            binding,
            batch,
            scope: RetirementScope::Binding,
        }
    }
    pub(crate) fn range(mut self, address: usize, size: usize) -> Self {
        self.scope = RetirementScope::Range { address, size };
        self
    }
    pub fn scope(&self) -> RetirementScope {
        self.scope
    }

    pub fn controller(&self) -> usize {
        self.controller
    }
    pub fn requester(&self) -> u32 {
        self.requester
    }
}

/// Bounded platform ordering evidence, installed by trusted boot code only.
/// This is a Rust boot interface, never a Unit acknowledgement or call ABI.
///
/// # Safety
/// The provider covers the immutable controller/requester and its entire real
/// isolation group. Returning a token proves that every earlier access that
/// could reach the retired range has completed, including requests not yet
/// observed by the IOMMU and required interconnect ordering. For Range, that
/// leaf is invalid and invalidation + IOFENCE.C PR/PW have completed; other
/// leaves and queues remain live. For Binding, DDTP is Off and all payloads,
/// context caches and translations have drained. Binding additionally proves
/// that old ownership cannot submit delayed requests into a replacement context.
/// The trusted integration must remove old raw device/configuration authority
/// before reassignment; a software epoch does not provide this isolation.
/// Bypass, ATS/PRI and noncoherent access are unsupported. No driver Ack, IRQ,
/// or business completion alone proves this contract. None/errors retain all
/// responsibilities. Each poll is bounded and cannot re-enable a context.
///
/// RISC-V IOMMU v20250828, "IOMMU Command-queue Fence commands": PR/PW orders
/// requests already processed by the IOMMU; platform ordering is independent.
/// https://docs.riscv.org/reference/iommu/v20250828/iommu_in_memory_queues.html
pub unsafe trait Quiescence: Send {
    fn prove(&mut self, retirement: Retirement) -> Result<Option<Retirement>, HardwareError>;
}

// SAFETY: exclusive forwarding preserves the installed provider's contract.
unsafe impl<Q: Quiescence + ?Sized> Quiescence for Box<Q> {
    fn prove(&mut self, retirement: Retirement) -> Result<Option<Retirement>, HardwareError> {
        (**self).prove(retirement)
    }
}

/// Diagnostic evidence from a failed external access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    pub device: u32,
    pub cause: u16,
    pub transaction: u8,
    pub address: u64,
}

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
pub(crate) unsafe trait Hardware<P: Copy> {
    /// Exact immutable boot binding; never supplied by a Unit.
    fn endpoint(&self) -> Option<crate::metal::generic::memory::Endpoint> {
        None
    }
    fn capacity(&self) -> usize {
        usize::MAX
    }
    /// Validate before retaining or publishing; must not change hardware state.
    fn validate(&self, _projection: Projection<P>) -> Result<(), HardwareError> {
        Ok(())
    }
    fn publish(&mut self, projection: Projection<P>) -> Result<(), HardwareError>;
    fn ready(&mut self, _projection: Projection<P>) -> Result<bool, HardwareError> {
        Ok(true)
    }
    fn revoke(&mut self, projection: Projection<P>) -> Result<(), HardwareError>;
    fn fence(&mut self, projection: Projection<P>, batch: BatchId) -> Result<(), HardwareError>;
    fn observe(&mut self, batch: BatchId) -> Result<Option<BatchId>, HardwareError>;
    /// Diagnostic evidence. A fault or an interrupt never completes a batch.
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
unsafe impl<P: Copy, H: Hardware<P> + ?Sized> Hardware<P> for Box<H> {
    fn endpoint(&self) -> Option<crate::metal::generic::memory::Endpoint> {
        (**self).endpoint()
    }
    fn capacity(&self) -> usize {
        (**self).capacity()
    }
    fn validate(&self, p: Projection<P>) -> Result<(), HardwareError> {
        (**self).validate(p)
    }
    fn publish(&mut self, p: Projection<P>) -> Result<(), HardwareError> {
        (**self).publish(p)
    }
    fn ready(&mut self, p: Projection<P>) -> Result<bool, HardwareError> {
        (**self).ready(p)
    }
    fn revoke(&mut self, p: Projection<P>) -> Result<(), HardwareError> {
        (**self).revoke(p)
    }
    fn fence(&mut self, p: Projection<P>, b: BatchId) -> Result<(), HardwareError> {
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

pub(crate) type Backend<P> = Box<dyn Hardware<P> + Send>;
