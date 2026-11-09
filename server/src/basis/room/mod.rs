//! Execution coordination: scheduling, deadlines, interrupt delivery and shutdown.
pub(crate) mod deadline;
pub(crate) mod interrupt;
pub(crate) mod scheduler;
pub(crate) mod shutdown;

pub enum Handoff<T> {
    Resume(T),
    Switch(usize),
}
