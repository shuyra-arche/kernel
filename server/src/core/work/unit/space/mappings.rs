//! 目标空间的 CPU 关系登记；仅显式控制记录授予 Memory 视图操作。
//! 来源与登记项共享帧无关绑定，真实页表与失效责任仍由 Nest 承担。

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::core::memory::nest::Mapping;
use crate::core::work::pend::SourceAuthorization;
use crate::machine::sync::SpinLock;
use kernel_api::{
    abi::call::pend::PendFail, model::identity::PendId, model::identity::TaskId,
    model::memory::MappingControl,
};

use super::{MappingBinding, MappingControlRecord, MappingEntry, Space};

pub(crate) struct MappingRegistry {
    entries: SpinLock<Vec<MappingEntry>>,
}

impl MappingRegistry {
    pub(crate) fn new() -> Self {
        Self {
            entries: SpinLock::new(Vec::new()),
        }
    }

    /// 占住这个号，并**同时**把这段关系登到来源名下：两表在同一个提交段里写。
    ///
    /// 可失败的准备（造绑定、查重号、两表各自备容量）全部先做完，之后两次短锁
    /// 只做写入——写进去就不再失败，不留只有一边的半条登记。
    pub(crate) fn claim(
        &self,
        id: usize,
        space: &Arc<Space>,
        controller: TaskId,
        scope: MappingControl,
        source: Arc<SourceAuthorization>,
        branch: Option<PendId>,
    ) -> Result<(), PendFail> {
        let control = MappingControlRecord::new(controller, scope, branch)?;
        self.claim_entry(id, space, source, Some(control))
    }

    pub(crate) fn claim_mapping(
        &self,
        id: usize,
        space: &Arc<Space>,
        source: Arc<SourceAuthorization>,
    ) -> Result<(), PendFail> {
        self.claim_entry(id, space, source, None)
    }

    fn claim_entry(
        &self,
        id: usize,
        space: &Arc<Space>,
        source: Arc<SourceAuthorization>,
        control: Option<Arc<MappingControlRecord>>,
    ) -> Result<(), PendFail> {
        let _commit = crate::core::work::commit();
        let binding = MappingBinding::new(id, space)?;
        {
            let mut entries = self.entries.lock();
            if entries.iter().any(|entry| entry.binding.id() == id) {
                return Err(PendFail::Busy);
            }
            entries.try_reserve(1).map_err(|_| PendFail::OoM)?;
        }
        source.reserve_binding()?;
        source.register_binding(binding.clone());
        self.entries.lock().push(MappingEntry {
            binding,
            control,
            mapping: None,
            source,
        });
        Ok(())
    }

    /// 提交：只把已备槽填上真实关系，不分配。
    pub(crate) fn commit(&self, id: usize, mapping: Arc<Mapping>) {
        let mut entries = self.entries.lock();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.binding.id() == id)
            .expect("prepared mapping registration");
        entry.mapping = Some(mapping);
    }

    /// 按号取回任意已登记关系；不以登记的存在代替控制授权。
    ///
    /// 不要拆成 `locate` + `pending` 两次查询：中间一旦提交，两次都会落空。
    pub(crate) fn registered(&self, id: usize) -> Option<MappingEntry> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.binding.id() == id)
            .cloned()
    }

    /// 按号取回可用控制；在途槽不算可用，号本身不授予任何东西。
    pub(crate) fn locate(&self, id: usize) -> Option<MappingEntry> {
        self.registered(id)
            .filter(|entry| entry.mapping.is_some() && entry.control.is_some())
    }

    /// 取回这一段关系的登记（连同它持有的关系与来源），由调用方在锁外结清。
    pub(crate) fn retire(&self, id: usize) -> Option<MappingEntry> {
        let mut entries = self.entries.lock();
        let at = entries.iter().position(|entry| entry.binding.id() == id)?;
        Some(entries.remove(at))
    }

    /// 取出下一笔 CPU 关系已结清的登记；占用身份与全空间关闭由 Nest 判定。
    ///
    /// 在途槽同样适用：发布失败的清理链走完后，它也要退表并释放 controller。
    pub(crate) fn take_settled(
        &self,
        nest: &crate::core::memory::nest::Nest,
    ) -> Option<MappingEntry> {
        let mut entries = self.entries.lock();
        let at = entries
            .iter()
            .position(|entry| nest.settled(entry.binding.id()))?;
        Some(entries.remove(at))
    }

    /// 分支改指：只改这一份来源自己那些仍带着被忘掉那枚 token 的记录。
    ///
    /// 授权快照与控制身份都不动——这里只改历史分支索引。

    /// 一份来源名下已登记的项（撤销时取出，在锁外关）。
    pub(crate) fn owned_by(
        &self,
        source: &Arc<SourceAuthorization>,
    ) -> Result<Vec<MappingEntry>, PendFail> {
        let entries = self.entries.lock();
        let mut result = Vec::new();
        result
            .try_reserve(entries.len())
            .map_err(|_| PendFail::OoM)?;
        result.extend(
            entries
                .iter()
                .filter(|entry| entry.mapping.is_some() && Arc::ptr_eq(&entry.source, source))
                .cloned(),
        );
        Ok(result)
    }
}

unsafe impl Send for MappingRegistry {}
unsafe impl Sync for MappingRegistry {}
