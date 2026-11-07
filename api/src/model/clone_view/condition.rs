#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CloneCondition {
    Pull,
    Push,
    Empty,
}
