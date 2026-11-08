//! 资源入口共用的那一手：本核当前任务按这次操作的许可取到这一枚，并判定它
//! 此刻是否还能承载这次操作。
//!
//! 数据和控制资格域各自直接调本域机制，只有身份与许可 look up
//! 以及「取到的这一枚还可用吗」共用。失败一律是 `PendFail`（权柄轴的状态），
//! 各域用自己的词汇显式折叠。

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::identity::PendId;
use kernel_api::model::view::ViewKind;

use crate::basis::pend;
use crate::basis::pend::Need;
use crate::basis::pend::PendSnapshot;
use crate::basis::room::conductor::current;
use crate::basis::view::share::ShareOperation;

pub(super) fn accede(token: PendId, need: Need) -> Result<PendSnapshot, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    pend::accede(&task, token, need)
}

/// 取到的这一枚此刻是否还能承载这次操作。
///
/// 交出中的不算可用；遗留的 heir 关系顺手清掉，让这一枚回到可操作状态。
pub(super) fn usable(pend: &PendSnapshot) -> Result<(), PendFail> {
    pend::available(pend)
}

/// 失活资源并关闭其关系；派生子图或在途发表未结时保留补关身份。
///
/// 核验与 `view.seal()` 必须在**同一个 pend** 里完成：与这次 Seal 抢同一份
/// 来源的迁移、撤销都在这把 pend 下串行，否则迁移（已核过授权）会在 Seal 的
/// 核验与失活之间把承载交出去。失活之后就放 pend——`close_maps` 要拆页、
/// 要 shootdown，不能带着 pend 做。
///
/// 关图失败（同步未确认 ⇒ `Busy`）时失活不回滚：这一枚还欠着关图，资源本身
/// 已失活时**原属主**可以对同一枚仍在表里的 token 重发 Seal 把关闭补完；资源
/// 仍活而这一枚的 Pend 已不答话，或换别的任务来，都照旧拒。
pub(super) fn seal_resource(token: PendId, kind: ViewKind) -> Result<usize, PendFail> {
    let me = current().running_task().ok_or(PendFail::Denied)?;
    let (share_view, _operation) = {
        let _serial = me.pend.lock();
        let pend = pend::locate(&me, token).ok_or(PendFail::Denied)?;
        if kind != pend.view().kind() {
            return Err(PendFail::Denied);
        }
        let share_view = pend.view().share_view();
        if !pend.alive()
            && !share_view
                .as_ref()
                .is_some_and(|meta| crate::basis::view::share::pending_close(meta))
        {
            return Err(PendFail::Dead);
        }
        // 补关资格只给“来源本身已失活、还欠着关图”那一档，且按 View 自己记的
        // 属主判：`PendEntry::owner()` 是 `alive().then(…)`，失活之后恒为 `None`，不给
        // 这条例外就没有人能把上面的图接着关完。来源仍活那一档照旧按这一枚的
        // 授权事实判——Pend 已不答话是授权失效，不是补关。两档都不恢复活权限。
        let cleanup = !pend.view().alive() && pend.view().owner() == me.ident.id;
        if pend.owner() != Some(me.ident.id) && !cleanup {
            return Err(PendFail::Denied);
        }
        let operation = if let Some(p) = &share_view {
            Some(ShareOperation::acquire(p).ok_or(PendFail::Busy)?)
        } else {
            None
        };
        let _commit = crate::basis::commit();
        pend.view().seal();
        (share_view, operation)
    };
    if let Some(share_view) = share_view {
        crate::basis::view::share::close(&share_view)?;
    }
    Ok(0)
}
