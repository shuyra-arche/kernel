//! 目标空间的 CPU 关系登记；仅显式控制记录授予 Memory 视图操作。
//! 来源与登记项共享帧无关绑定，真实页表与失效责任仍由 Nest 承担。

pub(crate) mod admission;
pub(crate) mod binding;
pub(crate) mod external;

use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use super::Span;
use crate::basis::pend::SourceAuthorization;
use crate::basis::unit::task::registry;
use crate::metal::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::cell::MappingControl;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;

use super::Nest;

// 目标空间与来源记录共享的 CPU 关系身份；不持帧，也不授予视图控制权。

pub(crate) struct RangeBinding {
    id: usize,
    nest: Weak<Nest>,
}

impl RangeBinding {
    pub(crate) fn new(id: usize, nest: &Arc<Nest>) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            id,
            nest: Arc::downgrade(nest),
        })
        .map_err(|_| PendFail::OutOfMemory)
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn nest(&self) -> &Weak<Nest> {
        &self.nest
    }
}

pub(crate) struct RangeControl {
    scope: MappingControl,
    control: SpinLock<(TaskId, Option<PendId>)>,
}

impl RangeControl {
    pub(crate) fn new(
        controller: TaskId,
        scope: MappingControl,
        branch: Option<PendId>,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            scope,
            control: SpinLock::new((controller, branch)),
        })
        .map_err(|_| PendFail::OutOfMemory)
    }

    pub(crate) fn scope(&self) -> MappingControl {
        self.scope
    }

    pub(crate) fn control(&self) -> (TaskId, Option<PendId>) {
        *self.control.lock()
    }
}

#[derive(Clone)]
pub(crate) struct RangeEntry {
    /// 与来源名下那条成员**同一个** `Arc`：控制身份与结清都只有这一处。
    pub(crate) binding: Arc<RangeBinding>,
    /// 资源映射没有 Memory 控制记录，不能凭关系号取得额外操作权。
    pub(crate) control: Option<Arc<RangeControl>>,
    pub(crate) mapping: Option<Arc<Span>>,
    pub(crate) source: Arc<SourceAuthorization>,
}

impl RangeEntry {
    pub(crate) fn mapping(&self) -> Option<&Arc<Span>> {
        self.mapping.as_ref()
    }
}

pub(crate) struct RangeRegistry {
    entries: SpinLock<Vec<RangeEntry>>,
}

impl RangeRegistry {
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
        nest: &Arc<Nest>,
        controller: TaskId,
        scope: MappingControl,
        source: Arc<SourceAuthorization>,
        branch: Option<PendId>,
    ) -> Result<(), PendFail> {
        let control = RangeControl::new(controller, scope, branch)?;
        self.claim_entry(id, nest, source, Some(control))
    }

    pub(crate) fn claim_mapping(
        &self,
        id: usize,
        nest: &Arc<Nest>,
        source: Arc<SourceAuthorization>,
    ) -> Result<(), PendFail> {
        self.claim_entry(id, nest, source, None)
    }

    fn claim_entry(
        &self,
        id: usize,
        nest: &Arc<Nest>,
        source: Arc<SourceAuthorization>,
        control: Option<Arc<RangeControl>>,
    ) -> Result<(), PendFail> {
        let _commit = crate::basis::commit();
        let binding = RangeBinding::new(id, nest)?;
        {
            let mut entries = self.entries.lock();
            if entries.iter().any(|entry| entry.binding.id() == id) {
                return Err(PendFail::Busy);
            }
            entries.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        }
        source.reserve_binding()?;
        source.register_binding(binding.clone());
        self.entries.lock().push(RangeEntry {
            binding,
            control,
            mapping: None,
            source,
        });
        Ok(())
    }

    /// 提交：只把已备槽填上真实关系，不分配。
    pub(crate) fn commit(&self, id: usize, mapping: Arc<Span>) {
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
    pub(crate) fn registered(&self, id: usize) -> Option<RangeEntry> {
        self.entries
            .lock()
            .iter()
            .find(|entry| entry.binding.id() == id)
            .cloned()
    }

    /// 按号取回可用控制；在途槽不算可用，号本身不授予任何东西。
    pub(crate) fn locate(&self, id: usize) -> Option<RangeEntry> {
        self.registered(id)
            .filter(|entry| entry.mapping.is_some() && entry.control.is_some())
    }

    /// 取回这一段关系的登记（连同它持有的关系与来源），由调用方在锁外结清。
    pub(crate) fn retire(&self, id: usize) -> Option<RangeEntry> {
        let mut entries = self.entries.lock();
        let at = entries.iter().position(|entry| entry.binding.id() == id)?;
        Some(entries.remove(at))
    }

    /// 取出下一笔 CPU 关系已结清的登记；占用身份与全空间关闭由 Nest 判定。
    ///
    /// 在途槽同样适用：发布失败的清理链走完后，它也要退表并释放 controller。
    pub(crate) fn take_settled(&self, nest: &Nest) -> Option<RangeEntry> {
        let mut entries = self.entries.lock();
        let at = entries
            .iter()
            .position(|entry| nest.settled(entry.binding.id()))?;
        Some(entries.remove(at))
    }

    /// 一份来源名下已登记的项（撤销时取出，在锁外关）。
    pub(crate) fn owned_by(
        &self,
        source: &Arc<SourceAuthorization>,
    ) -> Result<Vec<RangeEntry>, PendFail> {
        let entries = self.entries.lock();
        let mut result = Vec::new();
        result
            .try_reserve(entries.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        result.extend(
            entries
                .iter()
                .filter(|entry| entry.mapping.is_some() && Arc::ptr_eq(&entry.source, source))
                .cloned(),
        );
        Ok(result)
    }
}

unsafe impl Send for RangeRegistry {}
unsafe impl Sync for RangeRegistry {}

impl Nest {
    /// 一段登记结清后的**唯一收尾**：源侧成员与 Registry 项一起撤，
    /// 最后按登记身份放掉 controller；取出的 entry 在 Registry 锁外放掉它
    /// 持有的关系与来源授权。
    fn settle(entry: RangeEntry) {
        let id = entry.binding.id();
        entry.source.release_binding(&entry.binding);
        if let Some(control) = &entry.control
            && control.scope() == kernel_api::model::cell::MappingControl::Caller
        {
            let (controller, _) = control.control();
            if let Some(task) = registry::find(controller).and_then(|task| task.upgrade()) {
                task.controls.remove(id);
            }
        }
    }

    /// 一段已结清登记的收尾入口：在途（未提交）与已提交同样适用。
    ///
    /// Registry 项与源侧成员在同一个 work 提交段里一起撤；没结清就什么都不撤，
    /// 留给既有重试。
    pub(crate) fn retire_mapping(&self, id: usize) {
        let _commit = crate::basis::commit();
        if !self.settled(id) {
            return;
        }
        if let Some(entry) = self.mappings.retire(id) {
            Self::settle(entry);
        }
    }

    /// 把一个空间里已结清的登记都收掉；同样一条收尾，不再重复一份解除动作。
    pub(crate) fn reap_mappings(&self) {
        let _commit = crate::basis::commit();
        while let Some(entry) = self.mappings.take_settled(self) {
            Self::settle(entry);
        }
    }
}
