//! 本核的运行环：trap 返回之后，这一步决定接下来跑谁。

use crate::machine::stop::hush;

use super::current;
use super::fetch::fetch;

/// trap 返回时的下一步：返回下一个任务的帧地址。
pub(crate) fn run() -> usize {
    hush();
    crate::basis::unit::lifecycle::bury();
    let commit = crate::basis::commit();
    if current()
        .running_task()
        .is_some_and(|task| crate::basis::unit::lifecycle::take_doomed(task.ident.id).is_some())
    {
        drop(commit);
        return crate::basis::unit::lifecycle::quit();
    }
    let next = current().advance();
    drop(commit);
    match next {
        Some(pa) => pa,
        None => fetch(),
    }
}
