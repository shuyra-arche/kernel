use crate::basis::nest::Access;
use crate::basis::nest::Mapping;
use crate::basis::nest::MappingRange;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::SpaceKind;
use crate::basis::room::scheduler::roster;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::life::Life;
use alloc::sync::Arc;
use alloc::sync::Weak;
use kernel_api::address::VirtAddr;
mod builder;
mod mapping;

pub use builder::SpaceBuilder;
pub(crate) use mapping::MappingBinding;
pub(crate) use mapping::MappingEntry;
pub(crate) use mapping::MappingRegistry;

pub(crate) mod wait;

pub(crate) mod external;

/// 一个团队的地址空间：CPU 翻译（`Nest`）加一条生命周期线。
///
/// 来源准备与选址组合由调用方显式给出；这里只保留“本空间的用户半边”这一条
/// 访问纪律：地址窗口与页表权限同时成立才放行。
/// `mappings` 是新 ABI 的**控制授权记录**，不是页槽副本。
pub struct Space {
    pub(crate) nest: Arc<Nest>,
    pub(super) life: Arc<Life>,
    pub(crate) mappings: MappingRegistry,
    pub(crate) dma: crate::machine::iommu::retirement::Owner,
}

impl Space {
    /// 关闭同步成功后结清关系登记；ASID 归还失败仍保留号与根表供本入口重试。
    /// 已退休后不得再次 wipe，避免影响复用此 ASID 的空间。
    pub(crate) fn retire(&self) -> Result<(), NestError> {
        // Hardware records already belong to Kernel. Closing admission requires
        // neither successful CPU retirement nor a live requesting Unit.
        self.dma.close();
        if self.nest.retired() {
            self.reap_mappings();
            return Ok(());
        }
        self.nest.close()?;
        sign::wipe_space(self.nest.asid());
        self.reap_mappings();
        self.nest.retire()?;
        self.reap_mappings();
        Ok(())
    }

    /// 一段登记结清后的**唯一收尾**：源侧成员与 Registry 项一起撤，
    /// 最后按登记身份放掉 controller；取出的 entry 在 Registry 锁外放掉它
    /// 持有的关系与来源授权。
    fn settle(entry: MappingEntry) {
        let id = entry.binding.id();
        entry.source.release_binding(&entry.binding);
        if let Some(control) = &entry.control
            && control.scope() == kernel_api::model::cell::MappingControl::Caller
        {
            let (controller, _) = control.control();
            if let Some(task) = roster::muster(controller).and_then(|task| task.upgrade()) {
                task.release_control(id);
            }
        }
    }

    /// 一段已结清登记的收尾入口：在途（未提交）与已提交同样适用。
    ///
    /// Registry 项与源侧成员在同一个 work 提交段里一起撤；没结清就什么都不撤，
    /// 留给既有重试。
    pub(crate) fn retire_mapping(&self, id: usize) {
        let _commit = crate::basis::commit();
        if !self.nest.settled(id) {
            return;
        }
        if let Some(entry) = self.mappings.retire(id) {
            Self::settle(entry);
        }
    }

    /// 把一个空间里已结清的登记都收掉；同样一条收尾，不再重复一份解除动作。
    pub(crate) fn reap_mappings(&self) {
        let _commit = crate::basis::commit();
        while let Some(entry) = self.mappings.take_settled(&self.nest) {
            Self::settle(entry);
        }
    }

    /// 撤下一段关系；无论成败都按这段关系的整段身份放行等待者。
    pub(crate) fn withdraw(&self, selection: MappingRange) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.nest.detach(selection);
        sign::signal(key);
        result
    }

    /// 接管一笔已落地关系的回收；无论成败都按它的整段身份放行等待者。
    pub(crate) fn dispose(&self, mapping: Arc<Mapping>) -> Result<(), NestError> {
        let key = self.mapping_key(&mapping);
        let result = self.nest.dispose(mapping);
        sign::signal(key);
        result
    }

    /// 改选择窗口的当前权限；无论成败都按它的整段身份放行等待者。
    pub(crate) fn access(&self, selection: MappingRange, access: Access) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.nest.set_access(selection, access);
        sign::signal(key);
        result
    }

    /// 收窄选择窗口的权限；无论成败都按它的整段身份放行等待者。
    pub(crate) fn narrow(&self, selection: MappingRange, ceiling: Access) -> Result<(), NestError> {
        let key = self.mapping_key(selection.mapping());
        let result = self.nest.narrow(selection, ceiling);
        sign::signal(key);
        result
    }

    /// 这段关系整段的等待位：空间 ASID 与关系身份一起定它。
    fn mapping_key(&self, mapping: &Arc<Mapping>) -> WakeKey {
        WakeKey::Mapping {
            space: self.nest.asid(),
            mapping: mapping.identity(),
        }
    }

    pub fn kind(&self) -> SpaceKind {
        self.nest.kind()
    }

    pub fn life(&self) -> Weak<Life> {
        Arc::downgrade(&self.life)
    }

    /// 一段字节窗口是否整段落在用户可达的半边之内。
    pub(crate) fn user_range(at: VirtAddr, size: usize) -> bool {
        let ceiling = crate::machine::mmu::riscv::mode::upper().as_usize();
        at.as_usize() < ceiling
            && at
                .as_usize()
                .checked_add(size)
                .is_some_and(|end| end <= ceiling)
    }

    /// 用户半边的一次访问判定。
    pub(crate) fn validate_access(&self, at: VirtAddr, len: usize, need: Access) -> bool {
        Self::user_range(at, len) && self.nest.check_access(at, len, need)
    }

    pub(crate) fn copy_in(&self, dst: &mut [u8], at: VirtAddr) -> bool {
        Self::user_range(at, dst.len()) && self.nest.copy_from(at, dst, Access::READ)
    }

    pub(crate) fn copy_out(&self, src: &[u8], at: VirtAddr) -> bool {
        Self::user_range(at, src.len()) && self.nest.copy_to(at, src, Access::WRITE)
    }

    /// 取一段指令的首字节：取不到就没有可取的长度。
    pub(crate) fn instruction_byte(&self, at: VirtAddr) -> Option<u8> {
        let mut byte = [0];
        (Self::user_range(at, byte.len()) && self.nest.copy_from(at, &mut byte, Access::EXECUTE))
            .then_some(byte[0])
    }
}

unsafe impl Send for Space {}
unsafe impl Sync for Space {}

impl Drop for Space {
    fn drop(&mut self) {
        if !self.nest.retired() {
            sign::wipe_space(self.nest.asid());
        }
    }
}
