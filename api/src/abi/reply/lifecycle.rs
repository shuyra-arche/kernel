use crate::model::{
    clone_view::PullOutcome,
    identity::TaskId,
    unit::{ExitCause, JoinReply, TaskExit},
};

impl crate::abi::reply::FromTriple for JoinReply {
    fn from_triple(cause: usize, task: usize, reason: usize) -> Self {
        if cause == 0 {
            assert!(task == 0 && reason == 0, "invalid Pending");
            return Self::Pending;
        }
        if cause == 5 {
            assert!(task == 0 && reason == 0, "invalid Complete");
            return Self::Complete;
        }
        assert!(
            task > 0 && task <= isize::MAX as usize,
            "invalid TaskExit id"
        );
        let cause = match cause {
            1 => ExitCause::Reap,
            2 => ExitCause::Slay,
            3 => ExitCause::Cascade,
            4 => ExitCause::Fault,
            _ => panic!("invalid exit cause"),
        };
        Self::Reaped(TaskExit {
            task: TaskId::new(task),
            cause,
            reason,
        })
    }
}

impl crate::abi::reply::FromTriple for PullOutcome {
    fn from_triple(token: usize, len: usize, sender: usize) -> Self {
        assert!(
            token > 0 && token <= isize::MAX as usize && len > 0,
            "invalid payload delivery"
        );
        Self {
            token: crate::model::identity::PendId::new(token),
            len,
            sender: TaskId::new(sender),
        }
    }
}
