use alloc::sync::Arc;

use crate::basis::memory::nest::Mapping;
use crate::basis::work::pend::SourceAuthorization;

use super::{MappingBinding, MappingControlRecord};

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
