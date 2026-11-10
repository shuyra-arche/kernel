//! Caller snapshots for creating a Slot or a Memo.

use super::{Allocation, MappingTarget, Placement, ReferenceRange, SlotId, pages};
use crate::abi::{
    layout::{fetch_words, store_words},
    register::Wire,
};
use crate::{
    address::VirtAddr,
    model::{
        identity::{PendId, TeamId},
        modifiable::Modifiable,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    Slot(ReferenceRange),
    Pend {
        token: PendId,
        offset: usize,
        count: usize,
    },
}

/// Geometry and authority appropriate to each access domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoDestination {
    Internal {
        team: TeamId,
        placement: Placement,
        access: u64,
    },
    External {
        authority: PendId,
        address: VirtAddr,
        access: Modifiable,
    },
}

impl MemoDestination {
    pub fn target(self) -> MappingTarget {
        match self {
            Self::Internal { team, .. } => MappingTarget::Cpu(team),
            Self::External { authority, .. } => MappingTarget::Io(authority),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellCreate {
    Slot(Allocation),
    /// Reserve stable positions: Slot source starts Vacant; Pend source supplies Ready associations.
    /// Neither form publishes hardware access.
    Memo {
        source: CellSource,
        destination: MemoDestination,
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
                words[3] = a.level.get();
            }
            Self::Memo {
                source,
                destination,
            } => {
                let target = destination.target();
                let (placement, access) = match destination {
                    MemoDestination::Internal {
                        placement, access, ..
                    } => (placement.encode(), access),
                    MemoDestination::External {
                        address, access, ..
                    } => (
                        [2, address.get(), 4096],
                        if access == Modifiable::Yes { 6 } else { 2 },
                    ),
                };
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
                words[7..10].copy_from_slice(&placement);
                words[10] = access as usize;
            }
        }
        let mut bytes = [0; Self::WIDTH];
        store_words(words, &mut bytes);
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let w: [usize; 12] = fetch_words(bytes)?;
        match w[0] {
            0 if w[4..].iter().all(|w| *w == 0) => {
                let allocation = Allocation {
                    size: w[1],
                    align: w[2],
                    level: super::CellLevel::new(w[3])?,
                };
                allocation.bytes()?;
                Some(Self::Slot(allocation))
            }
            1 => {
                if w[2] != 0 || w[4] == 0 || w[4] > isize::MAX as usize {
                    return None;
                }
                pages(w[5], w[6])?;
                let target = MappingTarget::unpack(&[w[1], w[2], 0, 0, 0, 0], &mut 0).ok()?;
                let placement = Placement::decode([w[7], w[8], w[9]]).ok()?;
                if w[11] != 0 {
                    return None;
                }
                let destination = match target {
                    MappingTarget::Cpu(team) => MemoDestination::Internal {
                        team,
                        placement,
                        access: w[10] as u64,
                    },
                    MappingTarget::Io(authority) => {
                        let super::PlacementAt::Exact(address) = placement.at() else {
                            return None;
                        };
                        // External publication has one whole Cell, one exact address and caller authority.
                        if w[3] == 0 && w[5] != 0 {
                            return None;
                        }
                        let access = match w[10] {
                            2 => Modifiable::No,
                            6 => Modifiable::Yes,
                            _ => return None,
                        };
                        MemoDestination::External {
                            authority,
                            address,
                            access,
                        }
                    }
                };
                Some(Self::Memo {
                    destination,
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
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatedCell {
    Slot(SlotId),
    Memo { token: PendId, address: VirtAddr },
}

impl CreatedCell {
    /// Validate the untrusted/foreign reply boundary before constructing typed IDs.
    pub fn decode(kind: usize, id: usize, address: usize) -> Option<Self> {
        if id == 0 || id > isize::MAX as usize {
            return None;
        }
        match kind {
            0 if address == 0 => Some(Self::Slot(SlotId::new(id))),
            1 if address.is_multiple_of(4096) => Some(Self::Memo {
                token: PendId::new(id),
                address: VirtAddr::new(address),
            }),
            _ => None,
        }
    }
}

impl crate::abi::reply::FromTriple for CreatedCell {
    fn from_triple(kind: usize, id: usize, address: usize) -> Self {
        Self::decode(kind, id, address).expect("invalid created Cell reply")
    }
}
