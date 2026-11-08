use super::{fetch_words, store_words};
use crate::{
    abi::register::Wire,
    address::VirtAddr,
    model::{cell::*, identity::PendId},
};
impl CellObject {
    pub const WIDTH: usize = 32;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        store_words(self.words(), &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::of(fetch_words(bytes)?)
    }

    pub(crate) fn words(self) -> [usize; 4] {
        match self {
            Self::Slot(id) => [0, id.get(), 0, 0],
            Self::Memo { target, mapping } => {
                let mut words = [0; 6];
                target.pack(&mut words, &mut 0);
                [1, words[0], words[1], mapping.get()]
            }
        }
    }
    pub(crate) fn of(w: [usize; 4]) -> Option<Self> {
        match w {
            [0, id, 0, 0] if id != 0 && id <= isize::MAX as usize => {
                Some(Self::Slot(SlotId::new(id)))
            }
            [1, tag, 0, mapping] if mapping != 0 && mapping <= isize::MAX as usize => {
                Some(Self::Memo {
                    target: MappingTarget::unpack(&[tag, 0, 0, 0, 0, 0], &mut 0).ok()?,
                    mapping: MappingId::new(mapping),
                })
            }
            _ => None,
        }
    }
}
impl CellPosition {
    pub const WIDTH: usize = 48;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        let o = self.object.words();
        store_words(
            [o[0], o[1], o[2], o[3], self.page, self.generation as usize],
            &mut out,
        );
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [a, b, c, d, page, generation] = fetch_words(bytes)?;
        let object = CellObject::of([a, b, c, d])?;
        page.checked_mul(super::super::super::address::PAGE_SIZE)?;
        if matches!(object, CellObject::Slot(_)) && generation != 0 {
            return None;
        }
        Some(Self {
            object,
            page,
            generation: generation as u64,
        })
    }
}
impl CellHandoff {
    pub const WIDTH: usize = CellPosition::WIDTH * 2;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        bytes[..48].copy_from_slice(&self.source.to_bytes());
        bytes[48..].copy_from_slice(&self.target.to_bytes());
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        Some(Self {
            source: CellPosition::from_bytes(&bytes[..48])?,
            target: CellPosition::from_bytes(&bytes[48..])?,
        })
    }
}
impl CellCreate {
    pub const WIDTH: usize = 96;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut words = [0usize; 12];
        match self {
            Self::Slot(a) => {
                words[1] = a.size;
                words[2] = a.align;
            }
            Self::Memo {
                target,
                source,
                placement,
                access,
                control,
            } => {
                words[0] = 1;
                let mut target_words = [0; 6];
                target.pack(&mut target_words, &mut 0);
                words[1..3].copy_from_slice(&target_words[..2]);
                words[3..7].copy_from_slice(&match source {
                    CellSource::Slot(r) => [0, r.reference.get(), r.offset, r.count],
                    CellSource::Pend {
                        token,
                        offset,
                        count,
                    } => [1, token.get(), offset, count],
                });
                words[7..10].copy_from_slice(&placement.encode());
                words[10] = access as usize;
                let mut control_words = [0; 6];
                control.pack(&mut control_words, &mut 0);
                words[11] = control_words[0];
            }
        }
        let mut bytes = [0; Self::WIDTH];
        store_words(words, &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let w: [usize; 12] = fetch_words(bytes)?;
        match w[0] {
            0 if w[3..].iter().all(|w| *w == 0) => Some(Self::Slot(Allocation::new(w[1], w[2])?)),
            1 => {
                if w[2] != 0 || w[4] == 0 || w[4] > isize::MAX as usize {
                    return None;
                }
                super::super::super::model::cell::pages(w[5], w[6])?;
                Some(Self::Memo {
                    target: MappingTarget::unpack(&[w[1], w[2], 0, 0, 0, 0], &mut 0).ok()?,
                    source: match w[3] {
                        0 => CellSource::Slot(ReferenceRange {
                            reference: SlotId::new(w[4]),
                            offset: w[5],
                            count: w[6],
                        }),
                        1 => CellSource::Pend {
                            token: PendId::new(w[4]),
                            offset: w[5],
                            count: w[6],
                        },
                        _ => return None,
                    },
                    placement: Placement::decode([w[7], w[8], w[9]]).ok()?,
                    access: w[10] as u64,
                    control: MappingControl::unpack(&[w[11], 0, 0, 0, 0, 0], &mut 0).ok()?,
                })
            }
            _ => None,
        }
    }
}
impl CellInfo {
    pub const WIDTH: usize = 64;
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut out = [0; Self::WIDTH];
        store_words(
            [
                self.generation as usize,
                self.state as usize,
                self.page as usize,
                self.frozen as usize | ((self.external as usize) << 1),
                self.address.get(),
                self.fault_cause as usize,
                self.fault_address as usize,
                0,
            ],
            &mut out,
        );
        out
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let [
            generation,
            state,
            page,
            flags,
            address,
            fault_cause,
            fault_address,
            reserved,
        ] = fetch_words(bytes)?;
        if flags > 3 || reserved != 0 {
            return None;
        }
        Some(Self {
            generation: generation as u64,
            state: match state {
                0 => CellState::Vacant,
                1 => CellState::Ready,
                2 => CellState::Published,
                3 => CellState::Publishing,
                4 => CellState::Retracting,
                5 => CellState::Failed,
                _ => return None,
            },
            page: page as u64,
            frozen: flags & 1 != 0,
            external: flags & 2 != 0,
            address: VirtAddr::new(address),
            fault_cause: fault_cause as u64,
            fault_address: fault_address as u64,
        })
    }
}
impl crate::abi::reply::FromTriple for CreatedCell {
    fn from_triple(kind: usize, id: usize, address: usize) -> Self {
        assert!(
            kind <= 1 && id != 0 && id <= isize::MAX as usize,
            "invalid created Cell"
        );
        Self {
            kind: kind as u64,
            id: id as u64,
            address: VirtAddr::new(address),
        }
    }
}
