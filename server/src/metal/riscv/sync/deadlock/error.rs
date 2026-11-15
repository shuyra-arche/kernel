#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlockInitError {
    OutOfMemory,
    AlreadyInit,
}
