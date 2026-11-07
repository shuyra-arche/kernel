use alloc::sync::Arc;
use core::fmt;

use kernel_api::model::unit::ExitCause as TaskExitCause;

use crate::core::work::sign::{Ticket, WakeKey};
use crate::core::work::unit::task::WaitCall;
use crate::machine::hart::HartId;

use axis::TaskAxis;

use super::Task;

mod axis;
pub(crate) mod control;

use control::{Control, Phase, Transition};

/// 一个任务的状态机：一根轴（`TaskAxis`）加一条「暂停」标记。轴上的每个问题与
/// 每次迁移都住在这里；`Task` 只负责加锁与轴外的效果（发射、停泊、唤醒）。
pub struct TaskState {
    axis: TaskAxis,
    paused: bool,
}

impl TaskState {
    pub(crate) fn held() -> TaskState {
        TaskState {
            axis: TaskAxis::Held,
            paused: false,
        }
    }

    pub(crate) fn running(hart: HartId, ticks_left: u32) -> TaskState {
        TaskState {
            axis: TaskAxis::OnHart { hart, ticks_left },
            paused: false,
        }
    }

    pub(crate) fn ready() -> TaskState {
        TaskState {
            axis: TaskAxis::Ready { next: None },
            paused: false,
        }
    }

    pub(crate) fn reaped(cause: TaskExitCause, reason: usize) -> TaskState {
        TaskState {
            axis: TaskAxis::Reaped {
                cause,
                reason,
                next: None,
            },
            paused: false,
        }
    }

    pub(crate) fn doom(&mut self, hart: Option<HartId>, cause: TaskExitCause, reason: usize) {
        self.axis = TaskAxis::Doomed {
            hart,
            cause,
            reason,
        };
        self.paused = false;
    }

    pub(crate) fn block(&mut self, key: WakeKey, ticket: Ticket, wait: Option<WaitCall>) {
        self.axis = TaskAxis::Waiting {
            key,
            ticket,
            next: None,
            wait,
        };
    }

    pub(crate) fn paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn is_held(&self) -> bool {
        matches!(self.axis, TaskAxis::Held)
    }

    pub(crate) fn is_ready(&self) -> bool {
        matches!(self.axis, TaskAxis::Ready { .. })
    }

    pub(crate) fn is_waiting(&self) -> bool {
        matches!(self.axis, TaskAxis::Waiting { .. })
    }

    pub(crate) fn is_parked(&self) -> bool {
        matches!(self.axis, TaskAxis::Parked)
    }

    pub(crate) fn is_doomed(&self) -> bool {
        matches!(self.axis, TaskAxis::Doomed { .. })
    }

    pub(crate) fn is_reaped(&self) -> bool {
        matches!(self.axis, TaskAxis::Reaped { .. })
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.dying().is_some()
    }

    pub(crate) fn dying(&self) -> Option<(TaskExitCause, usize)> {
        match &self.axis {
            TaskAxis::Doomed { cause, reason, .. } | TaskAxis::Reaped { cause, reason, .. } => {
                Some((*cause, *reason))
            }
            _ => None,
        }
    }

    pub(crate) fn exit(&self) -> Option<(TaskExitCause, usize)> {
        match &self.axis {
            TaskAxis::Reaped { cause, reason, .. } => Some((*cause, *reason)),
            _ => None,
        }
    }

    pub(crate) fn on_hart(&self) -> Option<HartId> {
        match &self.axis {
            TaskAxis::OnHart { hart, .. }
            | TaskAxis::Doomed {
                hart: Some(hart), ..
            } => Some(*hart),
            _ => None,
        }
    }

    pub(crate) fn doomed_on_hart(&self) -> Option<usize> {
        match &self.axis {
            TaskAxis::Doomed {
                hart: Some(_),
                reason,
                ..
            } => Some(*reason),
            _ => None,
        }
    }

    pub(crate) fn ticks_left(&self) -> u32 {
        match &self.axis {
            TaskAxis::OnHart { ticks_left, .. } => *ticks_left,
            other => unreachable!("预算只在核上的任务身上，实为 {other:?}"),
        }
    }

    pub(crate) fn dec_ticks_left(&mut self) {
        let TaskAxis::OnHart { ticks_left, .. } = &mut self.axis else {
            unreachable!("dec_ticks_left 只对在核上的任务调用")
        };
        debug_assert!(*ticks_left >= 1, "既有预算恒 ≥ 1");
        *ticks_left -= 1;
    }

    pub(crate) fn wait_ticket(&self) -> Ticket {
        match &self.axis {
            TaskAxis::Waiting { ticket, .. } => *ticket,
            other => unreachable!("等待链只穿 Waiting 任务，实为 {other:?}"),
        }
    }

    pub(crate) fn take_wait(&mut self) -> Option<WaitCall> {
        match &mut self.axis {
            TaskAxis::Waiting { wait, .. } => wait.take(),
            _ => None,
        }
    }

    pub(crate) fn link(&mut self) -> &mut Option<Arc<Task>> {
        self.axis.chain()
    }

    /// 换轴：只有既有的合法边（见矩阵）才允许，且新轴不得带着入链载荷。
    pub(crate) fn transform(&mut self, next: TaskState) {
        let legal = match (&self.axis, &next.axis) {
            (TaskAxis::Held, TaskAxis::Ready { .. })
            | (TaskAxis::Ready { .. }, TaskAxis::OnHart { .. })
            | (TaskAxis::OnHart { .. }, TaskAxis::Ready { .. })
            | (TaskAxis::Doomed { .. }, TaskAxis::Reaped { .. }) => true,
            (old, TaskAxis::Doomed { .. }) => {
                !matches!(old, TaskAxis::Doomed { .. } | TaskAxis::Reaped { .. })
            }
            _ => false,
        };
        assert!(legal, "illegal task state transition");
        debug_assert!(
            next.axis.unlinked(),
            "transform_checked: 入链载荷非空（有人没先摘链）"
        );
        *self = next;
    }

    pub(crate) fn control(&self) -> Control {
        let phase = match self.axis {
            TaskAxis::Held => Phase::Held,
            TaskAxis::Ready { .. } => Phase::Ready,
            TaskAxis::OnHart { .. } => Phase::Running,
            TaskAxis::Waiting { .. } => Phase::Waiting,
            TaskAxis::Parked => Phase::Parked,
            TaskAxis::Doomed { .. } => Phase::Terminating,
            TaskAxis::Reaped { .. } => Phase::Finished,
        };
        Control {
            phase,
            paused: self.paused,
        }
    }

    /// Publish a decision only against the state from which it was computed.
    /// The caller holds the commit boundary through the external effect.
    pub(crate) fn apply_control(&mut self, transition: Transition) {
        assert_eq!(
            self.control(),
            transition.before,
            "stale control transition"
        );
        if transition.after.phase != transition.before.phase {
            assert_eq!(transition.before.phase, Phase::Parked);
            assert_eq!(transition.after.phase, Phase::Ready);
            self.axis = TaskAxis::Ready { next: None };
        }
        self.paused = transition.after.paused;
    }

    /// 停泊：暂停中的任务摆成就绪（等人把它捞走），否则进泊位。
    pub(crate) fn park(&mut self) {
        let paused = self.paused;
        self.axis = if paused {
            TaskAxis::Ready { next: None }
        } else {
            TaskAxis::Parked
        };
    }

    /// 升起：轴摆成就绪，回答「暂停」是否还挂着（挂着就由调用方送去泊位）。
    pub(crate) fn rise(&mut self) -> bool {
        self.axis = TaskAxis::Ready { next: None };
        self.paused
    }
}

impl fmt::Debug for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.axis)?;
        if self.paused {
            write!(f, " (paused)")?;
        }
        Ok(())
    }
}
