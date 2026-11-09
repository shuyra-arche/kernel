//! work 全域的提交锁：任务与团队的状态迁移、pend 的登记、view 的交接都在它下面
//! 串行。
//! Outer acquisitions queue in arrival order; recursion retains the same turn.
//! This prevents a releasing Hart or try_commit caller from overtaking waiters.
//! It bounds predecessor count, not the time spent in each transaction.

static COMMIT: crate::metal::sync::RelLock<()> =
    crate::metal::sync::RelLock::new_level(crate::metal::sync::Level::Unit, ());

#[cfg(debug_assertions)]
pub(crate) fn assert_unlocked() {
    assert!(
        !COMMIT.held_by_current(),
        "idle wait while holding Unit commit lock"
    );
}

/// 这一步必须在提交段里做：调用方自己取锁并把它一直持到发表完毕。
///
/// 只看“当场事实”的判据（如来源侧新使用判据）靠它把前提写死在代码里：
/// 单拿谓词去判、再在段外发表，中间会有人插进一条新占用。
#[cfg(debug_assertions)]
pub(crate) fn assert_held() {
    assert!(
        COMMIT.held_by_current(),
        "this step belongs inside the Unit commit section"
    );
}

/// A borrowed witness that the global transaction lock remains held.
/// Scheduler handoffs pass this guard between dequeue and seating.
pub(crate) type CommitGuard = crate::metal::sync::reentrant::RelLockGuard<'static, ()>;

#[cfg_attr(feature = "latency", track_caller)]
pub(crate) fn commit() -> CommitGuard {
    COMMIT.lock()
}

/// Interrupt work may retain its event and retry instead of waiting IRQ-off.
#[cfg_attr(feature = "latency", track_caller)]
pub(crate) fn try_commit() -> Option<CommitGuard> {
    COMMIT.try_lock()
}
