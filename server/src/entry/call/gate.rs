//! 资源入口共用的那一手：本核当前任务按这次操作的许可取到这一枚，并判定它
//! 此刻是否还能承载这次操作。
//!
//! 数据和控制资格域各自直接调本域机制，只有身份与许可 look up
//! 以及「取到的这一枚还可用吗」共用。失败一律是 `PendFail`（权柄轴的状态），
//! 各域用自己的词汇显式折叠。

use kernel_api::{abi::call::pend::PendFail, model::identity::PendId};

use crate::core::work::pend::{self, Need, PendSnapshot};
use crate::core::work::room::conductor::current;

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
