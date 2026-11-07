//! 设备地址投影的内核协议：登记、撤销批次与可信结清。
//!
//! 纯状态决策、Kernel 持有的回收记录与受独占访问保护的硬件效果。
//! 单设备适配器已由 QEMU 验证；普通启动仍无 DMA 授权 ABI 或队列中断接线。

mod binding;
mod error;
mod hardware;
mod identity;
mod phase;
mod projection;
pub(crate) mod retirement;
pub(crate) mod riscv;

use alloc::sync::Arc;
use alloc::vec::Vec;
use kernel_api::model::memory::PAGE_SIZE;

use crate::basis::memory::cell::Cell;
use binding::Binding;
use phase::Event;

pub(crate) use error::{Error, HardwareError};
pub(crate) use hardware::Hardware;
pub(crate) use identity::{BatchId, BindingId};
pub(crate) use phase::{Life, Phase};
pub(crate) use projection::Projection;

/// 记录留在 Kernel，与请求它们的 Unit 存活无关。&mut 是本协议的提交边界；
/// 接入并发调用时，外层必须提供同一锁/执行器，不能复制硬件适配器绕过本对象。
pub(crate) struct IoSpace<H: Hardware> {
    hardware: Option<H>,
    bindings: Vec<Binding>,
    life: Life,
    cursor: usize,
}

impl<H: Hardware> IoSpace<H> {
    pub(crate) fn new(hardware: H) -> Self {
        Self {
            hardware: Some(hardware),
            bindings: Vec::new(),
            life: Life::Open,
            cursor: 0,
        }
    }

    pub(crate) fn life(&self) -> Life {
        self.life
    }
    pub(crate) fn pending(&self) -> usize {
        self.bindings.len()
    }

    pub(crate) fn phase(&self, id: BindingId) -> Result<Phase, Error> {
        Ok(self.bindings[self.index(id)?].phase)
    }

    fn index(&self, id: BindingId) -> Result<usize, Error> {
        self.bindings
            .iter()
            .position(|binding| binding.projection.id == id)
            .ok_or(Error::UnknownBinding)
    }

    /// 登记不发布；成功后 Cell 已为 External，随后才允许硬件看见它。
    ///
    /// # Safety
    /// 调用方已验证设备域资格、Cell 范围借用以及与 CPU/其他设备的访问互斥，
    /// 并在关系存续期间保持 Flat 驻留/迁移约束。Arc 本身不能替代这些授权。
    pub(crate) unsafe fn register(
        &mut self,
        cell: &Arc<Cell>,
        iova: usize,
        modifiable: bool,
    ) -> Result<BindingId, Error> {
        if self.life != Life::Open {
            return Err(Error::Closed);
        }
        if self.pending() >= self.hardware.as_ref().expect("live space").capacity() {
            return Err(Error::Capacity);
        }
        let end = iova.checked_add(PAGE_SIZE).ok_or(Error::InvalidRange)?;
        if !iova.is_multiple_of(PAGE_SIZE) {
            return Err(Error::InvalidRange);
        }
        if self.bindings.iter().any(|binding| {
            let start = binding.projection.iova;
            iova < start + binding.projection.size && start < end
        }) {
            return Err(Error::Overlap);
        }
        self.bindings
            .try_reserve(1)
            .map_err(|_| Error::OutOfMemory)?;
        let id = BindingId::allocate()?;
        let projection = Projection {
            id,
            cell: cell.id(),
            iova,
            physical: cell.physical_address(),
            size: PAGE_SIZE,
            modifiable,
        };
        self.hardware
            .as_ref()
            .expect("live space")
            .validate(projection)?;
        let retained = cell.prepare_external(modifiable)?;
        self.bindings.push(Binding {
            projection,
            retention: Some(retained),
            phase: Phase::Prepared,
        });
        Ok(id)
    }

    pub(crate) fn publish(&mut self, id: BindingId) -> Result<(), Error> {
        if self.life != Life::Open {
            return Err(Error::Closed);
        }
        let at = self.index(id)?;
        let next = self.bindings[at].phase.transition(Event::Publish)?;
        let binding = &mut self.bindings[at];
        binding
            .retention
            .as_mut()
            .expect("registered retention")
            .publish();
        binding.phase = next;
        if let Err(error) = self
            .hardware
            .as_mut()
            .expect("live space")
            .publish(binding.projection)
        {
            // publish 可能已经写入部分 PTE；错误也不能按 Prepared 取消。
            binding.begin_retirement();
            binding.phase = binding.phase.transition(Event::Fail)?;
            return Err(error.into());
        }
        Ok(())
    }

    /// Publication is asynchronous. No caller may start DMA before ready.
    pub(crate) fn ready(&mut self, id: BindingId) -> Result<bool, Error> {
        if self.life != Life::Open {
            return Err(Error::Closed);
        }
        let at = self.index(id)?;
        if !matches!(self.bindings[at].phase, Phase::Publishing | Phase::Active) {
            return Err(Error::InvalidState);
        }
        match self
            .hardware
            .as_mut()
            .expect("live space")
            .ready(self.bindings[at].projection)
        {
            Ok(ready) => {
                if ready {
                    self.bindings[at].phase = self.bindings[at].phase.transition(Event::Ready)?;
                }
                Ok(ready)
            }
            Err(error) => {
                self.bindings[at].begin_retirement();
                self.bindings[at].phase = self.bindings[at].phase.transition(Event::Fail)?;
                Err(error.into())
            }
        }
    }

    pub(crate) fn fault(&mut self) -> Result<Option<riscv::Fault>, Error> {
        self.hardware
            .as_mut()
            .expect("live space")
            .fault()
            .map_err(Into::into)
    }

    /// 开始撤销或在失败后重试。Waiting 重复调用保留原批次；Failed 重试分配新批次。
    /// None 只用于尚未发布的取消，不产生硬件效果。
    pub(crate) fn revoke(&mut self, id: BindingId) -> Result<Option<BatchId>, Error> {
        let at = self.index(id)?;
        let phase = self.bindings[at].phase;
        if let Phase::Waiting(batch) = phase {
            return Ok(Some(batch));
        }
        let next = phase.transition(Event::Revoke)?;
        if next == Phase::Settled {
            self.bindings.remove(at);
            self.finish_close()?;
            return Ok(None);
        }
        // 身份耗尽不能让旧批次复活，也不能先消耗硬件效果再返回准备错误。
        let batch = BatchId::allocate()?;
        let binding = &mut self.bindings[at];
        if matches!(phase, Phase::Publishing | Phase::Active) {
            binding.begin_retirement();
        }
        binding.phase = next;
        let hardware = self.hardware.as_mut().expect("live space");
        let result = hardware
            .revoke(binding.projection)
            .and_then(|()| hardware.fence(binding.projection, batch));
        match result {
            Ok(()) => {
                binding.phase = binding.phase.transition(Event::Submitted(batch))?;
                Ok(Some(batch))
            }
            Err(error) => {
                binding.phase = binding.phase.transition(Event::Fail)?;
                Err(error.into())
            }
        }
    }

    /// 只向 Kernel 安装的适配器读取证据；没有接受普通 Unit Ack 的入口。
    pub(crate) fn poll(&mut self, id: BindingId) -> Result<bool, Error> {
        let at = self.index(id)?;
        let Phase::Waiting(batch) = self.bindings[at].phase else {
            return Err(Error::InvalidState);
        };
        let observed = self.hardware.as_mut().expect("live space").observe(batch);
        let seen = match observed {
            Ok(None) => return Ok(false),
            Ok(Some(seen)) => seen,
            Err(error) => {
                self.bindings[at].phase = self.bindings[at].phase.transition(Event::Fail)?;
                return Err(error.into());
            }
        };
        // 不同设备域、不同尝试或重复的迟到完成都不能结清本记录。
        self.bindings[at].phase.transition(Event::Complete(seen))?;
        let mut binding = self.bindings.remove(at);
        // SAFETY: trusted Hardware confirms the exact batch after revocation;
        // its contract includes all translations, in-flight accesses and platform fences.
        unsafe {
            binding
                .retention
                .take()
                .expect("retiring retention")
                .settle()
        };
        self.finish_close()?;
        Ok(true)
    }

    /// Unit 退出/正常关闭共用此入口：先关闭准入，再尽可能推进每条关系。
    /// 一条失败不阻止其他关系开始撤销；记录仍由 Kernel 保留，后续可重试。
    pub(crate) fn close(&mut self) -> Result<(), Error> {
        self.begin_close();
        let mut first_error = None;
        let mut at = 0;
        while at < self.bindings.len() {
            let id = self.bindings[at].projection.id;
            let before = self.bindings.len();
            if let Err(error) = self.revoke(id) {
                first_error.get_or_insert(error);
            }
            if self.bindings.len() == before {
                at += 1;
            }
        }
        self.finish_close()?;
        first_error.map_or(Ok(()), Err)
    }

    fn begin_close(&mut self) {
        if self.life == Life::Open {
            self.life = Life::Closing;
        }
    }

    /// One binding step per visit. Exit itself only marks admission closed;
    /// the Kernel registry calls this under its own serialization boundary.
    pub(crate) fn advance_close(&mut self) -> Result<bool, Error> {
        self.begin_close();
        if !self.bindings.is_empty() {
            self.cursor %= self.bindings.len();
            let id = self.bindings[self.cursor].projection.id;
            let phase = self.bindings[self.cursor].phase;
            self.cursor += 1;
            match phase {
                Phase::Waiting(_) => {
                    self.poll(id)?;
                }
                _ => {
                    self.revoke(id)?;
                }
            }
        }
        self.finish_close()?;
        Ok(self.life == Life::Closed)
    }

    fn finish_close(&mut self) -> Result<(), Error> {
        if self.life == Life::Closing
            && self.bindings.is_empty()
            && self.hardware.as_mut().expect("live space").shutdown()?
        {
            self.life = Life::Closed;
        }
        Ok(())
    }
}

impl<H: Hardware> Drop for IoSpace<H> {
    fn drop(&mut self) {
        if self
            .bindings
            .iter()
            .any(|binding| binding.phase != Phase::Prepared)
        {
            // 丢失整个管理器也不能释放仍可能被硬件读写的队列/页表。
            // Cell 凭证随后各自永久保留；正常失败应保留此 IoSpace 继续重试。
            core::mem::forget(self.hardware.take());
        }
    }
}
