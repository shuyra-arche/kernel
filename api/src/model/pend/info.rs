use super::PendStatus;
use crate::model::{
    identity::{Mark, PendId, TaskId},
    modifiable::Modifiable,
    view::{ViewId, ViewKind},
};
/// Eight whole words. A View identity is descriptive; the holder's Pend grants access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendInfo {
    pub token: PendId,
    pub view: ViewId,
    pub kind: ViewKind,
    pub modifiable: Modifiable,
    pub owner: TaskId,
    pub lender: TaskId,
    pub mark: Mark,
    pub status: PendStatus,
}
