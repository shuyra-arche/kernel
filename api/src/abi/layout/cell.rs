use super::{WORD_BYTES, fetch_words, store_words};
use crate::abi::register::Decode;
use crate::abi::register::Wire;
use crate::address::VirtAddr;
use crate::model::cell::Placement;
use crate::model::cell::PlacementAt;
use crate::model::cell::PlacementError;
use crate::model::cell::ReferenceRange;

impl ReferenceRange {
    /// 过线的字数；也是它在用户内存里作定长描述时的字数（同一份布局）。
    pub const WORDS: usize = 3;

    /// 把自身写进定长描述字；与 [`Wire`] 是同一份布局，不重写第二份。
    pub fn encode(self) -> [usize; Self::WORDS] {
        let mut buffer = [0usize; 6];
        let mut at = 0usize;
        self.pack(&mut buffer, &mut at);
        debug_assert_eq!(at, Self::WORDS);
        let mut words = [0usize; Self::WORDS];
        words.copy_from_slice(&buffer[..Self::WORDS]);
        words
    }

    /// 由定长描述字解码；同一份 checked 口径，非法位 → `Err(Decode)`。
    pub fn decode(words: [usize; Self::WORDS]) -> Result<Self, Decode> {
        let mut buffer = [0usize; 6];
        buffer[..Self::WORDS].copy_from_slice(&words);
        let mut at = 0usize;
        let value = Self::unpack(&buffer, &mut at)?;
        if at != Self::WORDS {
            return Err(Decode::Invalid);
        }
        Ok(value)
    }
}
impl ReferenceRange {
    pub const WIDTH: usize = Self::WORDS * WORD_BYTES;

    /// Write the fixed ABI descriptor. `out` must have exactly `WIDTH` bytes.
    pub fn store(&self, out: &mut [u8]) {
        store_words(self.encode(), out);
    }

    pub fn fetch(bytes: &[u8]) -> Option<Self> {
        Self::decode(fetch_words(bytes)?).ok()
    }
}

impl Placement {
    /// 描述在用户内存里的定长字数；`encode` / `decode` 的数组长度从这里取。
    pub const WORDS: usize = 3;

    /// 把自身编码成描述的那几个字（写入方在栈上备好，再交给内核快照）。
    pub fn encode(self) -> [usize; Self::WORDS] {
        let (kind, address) = match self.at() {
            PlacementAt::Lowest => (0, 0),
            PlacementAt::Highest => (1, 0),
            PlacementAt::Exact(address) => (2, address.get()),
        };
        [kind, address, self.align()]
    }

    /// 由描述字解码；方式越界 → [`PlacementError::Tag`]，
    /// 方式认得而对齐/定点非法 → [`PlacementError::Align`]。
    pub fn decode(words: [usize; Self::WORDS]) -> Result<Self, PlacementError> {
        let [kind, address, align] = words;
        let at = match kind {
            0 if address == 0 => PlacementAt::Lowest,
            1 if address == 0 => PlacementAt::Highest,
            2 => PlacementAt::Exact(VirtAddr::new(address)),
            _ => return Err(PlacementError::Tag),
        };
        Self::checked(at, align).ok_or(PlacementError::Align)
    }

    pub fn decode_bytes(bytes: &[u8]) -> Result<Self, PlacementError> {
        Self::decode(fetch_words(bytes).ok_or(PlacementError::Tag)?)
    }
}
impl Placement {
    pub const WIDTH: usize = Self::WORDS * WORD_BYTES;

    /// Write the fixed ABI descriptor. `out` must have exactly `WIDTH` bytes.
    pub fn store(&self, out: &mut [u8]) {
        store_words(self.encode(), out);
    }

    pub fn fetch(bytes: &[u8]) -> Option<Self> {
        Self::decode_bytes(bytes).ok()
    }
}

use crate::model::cell::{FlatReference, MappingId, Position};
use crate::model::identity::TeamId;
impl Position {
    pub const WIDTH: usize = 32;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let words = match self {
            Self::Slot { reference, page } => [0, reference.get() as u64, page as u64, 0],
            Self::Memo {
                team,
                mapping,
                page,
            } => [1, team.get() as u64, mapping.get() as u64, page as u64],
        };
        let mut bytes = [0; Self::WIDTH];
        for (word, out) in words.into_iter().zip(bytes.chunks_exact_mut(8)) {
            out.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let mut words = [0usize; 4];
        for (out, bytes) in words.iter_mut().zip(bytes.chunks_exact(8)) {
            *out = usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()?;
        }
        match words {
            [0, reference, page, 0] if reference != 0 => Some(Self::Slot {
                reference: FlatReference::from_bytes(&(reference as u64).to_le_bytes())?,
                page,
            }),
            [1, team, mapping, page] if mapping != 0 => Some(Self::Memo {
                team: TeamId::new(team),
                mapping: MappingId::from_bytes(&(mapping as u64).to_le_bytes())?,
                page,
            }),
            _ => None,
        }
    }
}

use crate::model::cell::{Attachment, MappingControl};
impl Attachment {
    pub const WIDTH: usize = 72;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        self.range.store(&mut bytes[..24]);
        self.placement.store(&mut bytes[24..48]);
        bytes[48..56].copy_from_slice(&self.access.to_le_bytes());
        store_words(
            [
                match self.control {
                    MappingControl::Caller => 0,
                    MappingControl::SpaceMembers => 1,
                },
                self.change,
            ],
            &mut bytes[56..],
        );
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let [control, change] = fetch_words(&bytes[56..])?;
        Some(Self {
            range: ReferenceRange::fetch(&bytes[..24])?,
            placement: Placement::decode_bytes(&bytes[24..48]).ok()?,
            access: u64::from_le_bytes(bytes[48..56].try_into().ok()?),
            control: match control {
                0 => MappingControl::Caller,
                1 => MappingControl::SpaceMembers,
                _ => return None,
            },
            change,
        })
    }
}
