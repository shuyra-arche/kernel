use kernel_api::model::identity::TaskId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Dormant,
    Armed { owner: TaskId },
    Tripped,
}
