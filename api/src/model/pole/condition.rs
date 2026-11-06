use super::Bit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PoleCondition {
    Signal(Bit),
}
