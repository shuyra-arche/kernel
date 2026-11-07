use crate::abi::call::sign::SignEventFail;
use crate::model::sign::SignSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignReply {
    Pending,
    Triggered,
    Source {
        source: SignSource,
        fail: Option<SignEventFail>,
    },
}

impl SignReply {
    pub fn is_none(self) -> bool {
        self == Self::Pending
    }

    pub fn words(self) -> [usize; 3] {
        match self {
            Self::Pending => [0; 3],
            Self::Triggered => [9, 0, 0],
            Self::Source { source, fail } => {
                let mut words = source.words();
                words[0] |=
                    fail.map_or(0, |e| (-crate::abi::error::FailCode::code(e)) as usize) << 8;
                words
            }
        }
    }

    pub fn of(mut words: [usize; 3]) -> Option<Self> {
        if words == [0; 3] {
            return Some(Self::Pending);
        }
        if words == [9, 0, 0] {
            return Some(Self::Triggered);
        }
        let fail = match words[0] >> 8 {
            0 => None,
            1 => Some(SignEventFail::Denied),
            2 => Some(SignEventFail::Dead),
            3 => Some(SignEventFail::Busy),
            4 => Some(SignEventFail::OoM),
            5 => Some(SignEventFail::HandedOver),
            6 => Some(SignEventFail::Gone),
            _ => return None,
        };
        words[0] &= 255;
        Some(Self::Source {
            source: SignSource::of(words)?,
            fail,
        })
    }
}

impl crate::abi::reply::FromTriple for SignReply {
    fn from_triple(a: usize, b: usize, c: usize) -> Self {
        Self::of([a, b, c]).expect("invalid Sign Listen reply")
    }
}
