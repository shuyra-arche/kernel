use kernel_api::model::identity::TeamId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Relation {
    Input,
    Open,
    Mapping,
    Staged(TeamId),
}
