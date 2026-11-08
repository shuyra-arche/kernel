//! Caller snapshots for creating a Slot or a Memo.

use super::{Allocation, MappingControl, MappingTarget, Placement, ReferenceRange, SlotId, pages};
use crate::abi::{
    layout::{fetch_words, store_words},
    register::Wire,
};
use crate::{address::VirtAddr, model::identity::PendId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    Slot(ReferenceRange),
    Pend {
        token: PendId,
        offset: usize,
        count: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellCreate {
    Slot(Allocation),
    /// Reserve stable positions: Slot source starts Vacant; Pend source supplies Ready associations.
    /// Neither form publishes hardware access.
    Memo {
        target: MappingTarget,
        source: CellSource,
        placement: Placement,
        access: u64,
        control: MappingControl,
    },
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
                pages(w[5], w[6])?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatedCell {
    pub kind: u64,
    pub id: u64,
    pub address: VirtAddr,
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
