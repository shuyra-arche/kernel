//! Monotonic deadlines use the same nanosecond epoch as Room.Clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    Poll,
    At(u64),
    Infinite,
}
