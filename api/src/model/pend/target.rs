//! Authorization identity is separate from the target object's identity.
use super::Operations;
use crate::model::sign::SignId;
use crate::model::{flat::FlatId, nest::NestId, view::ViewId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewTarget {
    Clone(ViewId),
    Share(ViewId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    None,
    View(ViewTarget),
    Flat(FlatId),
    Nest(NestId),
    Sign(SignId),
}
impl Target {
    pub const fn words(self) -> [usize; 2] {
        match self {
            Self::None => [0, 0],
            Self::View(ViewTarget::Clone(id)) => [1, id.get()],
            Self::View(ViewTarget::Share(id)) => [2, id.get()],
            Self::Flat(id) => [3, id.get()],
            Self::Nest(id) => [4, id.get()],
            Self::Sign(id) => [6, id.get()],
        }
    }
    pub fn of([kind, id]: [usize; 2]) -> Option<Self> {
        if kind == 0 {
            return (id == 0).then_some(Self::None);
        }
        if id == 0 || id > isize::MAX as usize {
            return None;
        }
        Some(match kind {
            1 => Self::View(ViewTarget::Clone(ViewId::new(id))),
            2 => Self::View(ViewTarget::Share(ViewId::new(id))),
            3 => Self::Flat(FlatId::new(id)?),
            4 => Self::Nest(NestId::new(id)?),
            // Kind 5 was Port and remains invalid.
            6 => Self::Sign(SignId::new(id)),
            _ => return None,
        })
    }
    pub fn accepts(self, operations: Operations) -> bool {
        self.words()[0] == operations.words()[0]
    }
}
