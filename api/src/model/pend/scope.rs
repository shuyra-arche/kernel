//! A Pend's object or page window. Reservation identities describe a scope, not authority.

use super::PendRange;
use crate::model::nest::ReservationId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestScope {
    Whole,
    Reservation { id: ReservationId, range: PendRange },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendScope {
    Object,
    Pages(PendRange),
    Nest(NestScope),
}

impl PendScope {
    pub const WORDS: usize = 5;

    pub const fn range(self) -> Option<PendRange> {
        match self {
            Self::Pages(range) | Self::Nest(NestScope::Reservation { range, .. }) => Some(range),
            Self::Object | Self::Nest(NestScope::Whole) => None,
        }
    }

    pub const fn words(self) -> [usize; Self::WORDS] {
        let (tag, id, range) = match self {
            Self::Object => return [0; Self::WORDS],
            Self::Pages(range) => (1, 0, range),
            Self::Nest(NestScope::Whole) => return [2, 0, 0, 0, 0],
            Self::Nest(NestScope::Reservation { id, range }) => (3, id.get(), range),
        };
        let [range_tag, offset, count] = range.words();
        [tag, id, range_tag, offset, count]
    }

    pub fn of(words: [usize; Self::WORDS]) -> Option<Self> {
        match words {
            [0, 0, 0, 0, 0] => Some(Self::Object),
            [1, 0, tag, offset, count] => Some(Self::Pages(PendRange::of([tag, offset, count])?)),
            [2, 0, 0, 0, 0] => Some(Self::Nest(NestScope::Whole)),
            [3, id, tag, offset, count] if id != 0 && id <= isize::MAX as usize => {
                Some(Self::Nest(NestScope::Reservation {
                    id: ReservationId::new(id),
                    range: PendRange::of([tag, offset, count])?,
                }))
            }
            _ => None,
        }
    }

    pub(super) fn same_kind(self, other: Self) -> bool {
        match (self, other) {
            (Self::Object, Self::Object)
            | (Self::Pages(_), Self::Pages(_))
            | (Self::Nest(NestScope::Whole), Self::Nest(NestScope::Whole)) => true,
            (
                Self::Nest(NestScope::Reservation { id, .. }),
                Self::Nest(NestScope::Reservation { id: other, .. }),
            ) => id == other,
            _ => false,
        }
    }
}
