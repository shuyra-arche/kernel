//! 目标空间的 CPU 关系登记；仅显式控制记录授予 Memory 视图操作。
//! 来源与登记项共享帧无关绑定，真实页表与失效责任仍由 Nest 承担。

use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use crate::basis::nest::Mapping;
use crate::basis::pend::SourceAuthorization;
use crate::machine::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::cell::MappingControl;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use super::Space;

// 目标空间与来源记录共享的 CPU 关系身份；不持帧，也不授予视图控制权。

pub(crate) struct MappingBinding {
    id: usize,
    space: Weak<Space>,
}

impl MappingBinding {
    pub(crate) fn new(id: usize, space: &Arc<Space>) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            id,
            space: Arc::downgrade(space),
        })
        .map_err(|_| PendFail::OoM)
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn space(&self) -> &Weak<Space> {
        &self.space
    }
}

pub(crate) struct MappingControlRecord {
    scope: MappingControl,
    control: SpinLock<(TaskId, Option<PendId>)>,
}

impl MappingControlRecord {
    pub(crate) fn new(
        controller: TaskId,
        scope: MappingControl,
        branch: Option<PendId>,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            scope,
            control: SpinLock::new((controller, branch)),
        })
        .map_err(|_| PendFail::OoM)
    }

    pub(crate) fn scope(&self) -> MappingControl {
        self.scope
    }

    pub(crate) fn control(&self) -> (TaskId, Option<PendId>) {
        *self.control.lock()
    }
}

#[derive(Clone)]
pub(crate) struct MappingEntry {
    /// 与来源名下那条成员**同一个** `Arc`：控制身份与结清都只有这一处。
    pub(crate) binding: Arc<MappingBinding>,
    /// 资源映射没有 Memory 控制记录，不能凭关系号取得额外操作权。
    pub(crate) control: Option<Arc<MappingControlRecord>>,
    pub(crate) mapping: Option<Arc<Mapping>>,
    pub(crate) source: Arc<SourceAuthorization>,
}

impl MappingEntry {
    pub(crate) fn mapping(&self) -> Option<&Arc<Mapping>> {
        self.mapping.as_ref()
    }
}

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
        let _commit = crate::basis::commit();
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
    pub(crate) fn take_settled(&self, nest: &crate::basis::nest::Nest) -> Option<MappingEntry> {
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
