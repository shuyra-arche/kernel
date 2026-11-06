use crate::model::identity::{Mark, PieToken, TaskId};
use crate::model::permission::Permission;
use crate::model::pie::PieKind;

/// Seven whole words; no Rust struct padding crosses the ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieInfo {
    pub token: PieToken,
    pub kind: PieKind,
    pub permission: Permission,
    pub owner: TaskId,
    pub vestor: TaskId,
    pub mark: Mark,
    pub alive: bool,
}
