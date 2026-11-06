use alloc::sync::Weak;
use kernel_api::model::identity::TaskId;

use crate::core::work::unit::life::Life;

pub(crate) struct TaskLife {
    pub(crate) id: TaskId,
    pub(crate) life: Weak<Life>,
}
