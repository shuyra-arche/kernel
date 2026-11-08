use crate::model::{clone_view::PullOutcome, identity::TaskId};
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
