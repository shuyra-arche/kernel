use super::chain::Link;
use crate::basis::unit::control::Request;
use core::fmt;
use kernel_api::abi::call::unit::UnitFail;

use kernel_api::model::unit::ExitCause as TaskExitCause;

use crate::basis::sign::Ticket;
use crate::basis::sign::WakeKey;
mod continuation;

use crate::metal::hart::HartId;
pub(crate) use continuation::{Resumption, WaitCall};

/// 任务状态机的轴：任一时刻只在一根轴上。`link` 是入链载荷，只挂在就绪 / 等待 /
/// 躯壳三种轴上（饥饿链、等待链、躯壳链共用同一个槽）。
pub enum TaskAxis {
    OnHart {
        hart: HartId,
        ticks_left: u32,
    },
    Ready {
        link: Link,
    },
    Waiting {
        key: WakeKey,
        ticket: Ticket,
        link: Link,
        wait: Option<WaitCall>,
    },
    Held,
    Parked,
    Doomed {
        hart: Option<HartId>,
        cause: TaskExitCause,
        reason: usize,
    },
    Reaped {
        cause: TaskExitCause,
        reason: usize,
        link: Link,
    },
}

impl TaskAxis {
    /// 入链载荷槽：链只穿就绪 / 等待 / 躯壳三种轴，其余轴没有槽。
    pub(crate) fn chain(&mut self) -> &mut Link {
        match self {
            TaskAxis::Ready { link }
            | TaskAxis::Waiting { link, .. }
            | TaskAxis::Reaped { link, .. } => link,
            other => unreachable!("链只穿就绪/等待/躯壳任务，实为 {other:?}"),
        }
    }

    /// 入链载荷是否为空：没有槽的轴恒为空。
    pub(crate) fn unlinked(&self) -> bool {
        match self {
            TaskAxis::Ready { link }
            | TaskAxis::Waiting { link, .. }
            | TaskAxis::Reaped { link, .. } => link.detached(),
            _ => true,
        }
    }
}

impl fmt::Debug for TaskAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskAxis::OnHart { hart, ticks_left } => write!(
                f,
                "OnHart {{ hart: {}, ticks_left: {ticks_left} }}",
                hart.get()
            ),
            TaskAxis::Ready { link } => write!(f, "Ready {{ attached: {} }}", !link.detached()),
            TaskAxis::Waiting {
                key, ticket, link, ..
            } => write!(
                f,
                "Waiting {{ key: {key:?}, ticket: {ticket:?}, attached: {} }}",
                !link.detached()
            ),
            TaskAxis::Held => write!(f, "Held"),
            TaskAxis::Parked => write!(f, "Parked"),
            TaskAxis::Doomed {
                hart,
                cause,
                reason,
            } => write!(
                f,
                "Doomed {{ hart: {}, cause: {cause:?}, reason: {reason:#x} }}",
                hart.map_or(0, HartId::get)
            ),
            TaskAxis::Reaped { cause, reason, .. } => {
                write!(f, "Reaped {{ cause: {cause:?}, reason: {reason:#x} }}")
            }
        }
    }
}

// Pure execution-control decisions. Queue, hart and notification effects are
// applied by Task while holding the work commit boundary.

mod phase;
pub(crate) use phase::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Control {
    pub phase: Phase,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Effect {
    None,
    Release,
    Enqueue,
    Dequeue,
    Interrupt,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Transition {
    pub before: Control,
    pub after: Control,
    pub effect: Effect,
}

impl Control {
    /// Compute the next state and required external action without changing
    /// either the Task or its execution environment.
    pub(crate) fn transition(self, request: Request) -> Result<Transition, UnitFail> {
        let (after, effect) = match request {
            Request::Pause => match self.phase {
                Phase::Terminating | Phase::Finished => return Err(UnitFail::Denied),
                Phase::Running if self.paused => return Err(UnitFail::Busy),
                _ if self.paused => (self, Effect::None),
                Phase::Running => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::Interrupt,
                ),
                Phase::Ready => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::Dequeue,
                ),
                Phase::Parked => (
                    Self {
                        phase: Phase::Ready,
                        paused: true,
                    },
                    Effect::None,
                ),
                Phase::Held | Phase::Waiting => (
                    Self {
                        paused: true,
                        ..self
                    },
                    Effect::None,
                ),
            },
            Request::Resume => {
                if !self.paused {
                    return Err(UnitFail::Busy);
                }
                match self.phase {
                    Phase::Terminating | Phase::Finished => return Err(UnitFail::Denied),
                    Phase::Waiting => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::None,
                    ),
                    Phase::Held => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::Release,
                    ),
                    Phase::Ready => (
                        Self {
                            paused: false,
                            ..self
                        },
                        Effect::Enqueue,
                    ),
                    Phase::Running | Phase::Parked => return Err(UnitFail::Busy),
                }
            }
        };
        Ok(Transition {
            before: self,
            after,
            effect,
        })
    }
}

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
            axis: TaskAxis::Ready {
                link: Link::Detached,
            },
            paused: false,
        }
    }

    pub(crate) fn reaped(cause: TaskExitCause, reason: usize) -> TaskState {
        TaskState {
            axis: TaskAxis::Reaped {
                cause,
                reason,
                link: Link::Detached,
            },
            paused: false,
        }
    }

    pub(crate) fn doom(&mut self, hart: Option<HartId>, cause: TaskExitCause, reason: usize) {
        if let Some((old_cause, old_reason)) = self.dying() {
            assert_eq!(
                (cause, reason),
                (old_cause, old_reason),
                "exit cause is immutable"
            );
        }
        self.set_axis(TaskAxis::Doomed {
            hart,
            cause,
            reason,
        });
        self.paused = false;
    }

    pub(crate) fn block(&mut self, key: WakeKey, ticket: Ticket, wait: Option<WaitCall>) {
        self.set_axis(TaskAxis::Waiting {
            key,
            ticket,
            link: Link::Detached,
            wait,
        });
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

    pub(in super::super) fn link(&mut self) -> &mut Link {
        self.axis.chain()
    }

    /// 换轴：只有既有的合法边（见矩阵）才允许，且新轴不得带着入链载荷。
    pub(crate) fn transform(&mut self, next: TaskState) {
        self.set_axis(next.axis);
        self.paused = next.paused;
    }

    pub(crate) fn control(&self) -> Control {
        Control {
            phase: self.axis.phase(),
            paused: self.paused,
        }
    }

    fn set_axis(&mut self, axis: TaskAxis) {
        assert!(
            self.axis.phase().allows(axis.phase()),
            "illegal task lifecycle transition"
        );
        assert!(
            self.axis.unlinked() && axis.unlinked(),
            "transition requires detached chain"
        );
        if let TaskAxis::Waiting { wait, .. } = &self.axis {
            assert!(
                wait.is_none(),
                "consume or cancel continuation before transition"
            );
        }
        if let Some(old) = self.dying() {
            let (TaskAxis::Doomed { cause, reason, .. } | TaskAxis::Reaped { cause, reason, .. }) =
                &axis
            else {
                unreachable!()
            };
            assert_eq!(old, (*cause, *reason), "exit cause is immutable");
        }
        self.axis = axis;
    }
}

impl TaskAxis {
    fn phase(&self) -> Phase {
        match self {
            TaskAxis::Held => Phase::Held,
            TaskAxis::Ready { .. } => Phase::Ready,
            TaskAxis::OnHart { .. } => Phase::Running,
            TaskAxis::Waiting { .. } => Phase::Waiting,
            TaskAxis::Parked => Phase::Parked,
            TaskAxis::Doomed { .. } => Phase::Terminating,
            TaskAxis::Reaped { .. } => Phase::Finished,
        }
    }
}
impl TaskState {
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
            self.set_axis(TaskAxis::Ready {
                link: Link::Detached,
            });
        }
        self.paused = transition.after.paused;
    }

    /// 停泊：暂停中的任务摆成就绪（等人把它捞走），否则进泊位。
    pub(crate) fn park(&mut self) {
        let paused = self.paused;
        self.set_axis(if paused {
            TaskAxis::Ready {
                link: Link::Detached,
            }
        } else {
            TaskAxis::Parked
        });
    }

    /// 升起：轴摆成就绪，回答「暂停」是否还挂着（挂着就由调用方送去泊位）。
    pub(crate) fn rise(&mut self) -> bool {
        self.set_axis(TaskAxis::Ready {
            link: Link::Detached,
        });
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
