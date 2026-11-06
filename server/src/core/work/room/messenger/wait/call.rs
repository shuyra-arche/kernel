#[derive(Clone)]
pub(crate) enum WaitCall {
    Value { deadline: Option<u64> },
    Join(crate::core::work::unit::join::JoinWait),
    Mail(crate::core::work::mail::tole::AwaitWait),
}
