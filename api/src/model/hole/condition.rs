#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HoleCondition {
    Pull,
    Push,
    Empty,
    Signal,
}
