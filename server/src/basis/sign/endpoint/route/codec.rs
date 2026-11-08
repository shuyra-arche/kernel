use alloc::sync::Weak;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::abi::call::sign::SignEventFail;
use kernel_api::abi::call::sign::SignFail;
use kernel_api::model::identity::PendId;
use kernel_api::model::sign::SignSource;

use crate::basis::pend::Need;
use crate::basis::pend::PendEntry;
use crate::basis::sign::WakeKey;
use crate::basis::unit::anchor::Anchor;
use crate::basis::view::clone;

/// 权柄轴的失败折成组这一域的操作词汇。
pub(crate) fn pend_fail(error: PendFail) -> SignFail {
    match error {
        PendFail::Denied | PendFail::NotAligned | PendFail::Invalid => SignFail::Denied,
        PendFail::Dead => SignFail::Dead,
        PendFail::OutOfMemory => SignFail::OutOfMemory,
        PendFail::HandedOver => SignFail::HandedOver,
        PendFail::Busy => SignFail::Busy,
    }
}

/// 事件面上的权柄失败：与操作面分开，失权、死亡与交接各自保留一格。
pub(crate) fn event_fail(error: PendFail) -> SignEventFail {
    match error {
        PendFail::Denied | PendFail::NotAligned | PendFail::Invalid => SignEventFail::Denied,
        PendFail::Dead => SignEventFail::Dead,
        PendFail::OutOfMemory => SignEventFail::OutOfMemory,
        PendFail::HandedOver => SignEventFail::HandedOver,
        PendFail::Busy => SignEventFail::Busy,
    }
}

/// 这份 source 看的是哪一枚、要哪一种许可；组内那两种非资源 source 没有资源面。
pub(crate) fn source_watch(source: SignSource) -> Option<(PendId, Need)> {
    match source {
        SignSource::Clone { token, condition } => Some((token, clone::need(condition))),
        SignSource::Sign { .. }
        | SignSource::Join { .. }
        | SignSource::Inspect { .. }
        | SignSource::MappingRetired { .. }
        | SignSource::MappingFault { .. } => None,
    }
}

/// 这份 source 落在哪个资源的哪条通知位上；不是这一族的资源就没有键。
pub(crate) fn source_key(
    pend_entry: &PendEntry,
    source: SignSource,
) -> Option<(WakeKey, Weak<Anchor>)> {
    if !pend_entry.permits(kernel_api::model::pend::Operations::Clone(
        kernel_api::model::pend::CloneRights::PEEK,
    )) {
        return None;
    }
    let view = pend_entry.target();
    match source {
        SignSource::Clone { condition, .. } => {
            let meta = view.clone_view()?;
            Some((clone::key(&meta, condition), meta.life()))
        }
        SignSource::Sign { .. }
        | SignSource::Join { .. }
        | SignSource::Inspect { .. }
        | SignSource::MappingRetired { .. }
        | SignSource::MappingFault { .. } => None,
    }
}

/// 这份 source 此刻是否已就绪：问它自己的资源机制。
pub(crate) fn source_ready(pend_entry: &PendEntry, source: SignSource) -> Option<bool> {
    if !pend_entry.permits(kernel_api::model::pend::Operations::Clone(
        kernel_api::model::pend::CloneRights::PEEK,
    )) {
        return None;
    }
    let view = pend_entry.target();
    match source {
        SignSource::Clone { condition, .. } => Some(view.clone_view()?.ready(condition)),
        SignSource::Sign { .. }
        | SignSource::Join { .. }
        | SignSource::Inspect { .. }
        | SignSource::MappingRetired { .. }
        | SignSource::MappingFault { .. } => None,
    }
}
