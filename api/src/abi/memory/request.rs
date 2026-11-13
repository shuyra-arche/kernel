//! Bounded caller-memory requests for the settled Flat/Nest transitions. The
//! header names the domain and operation; unused bytes have one canonical zero
//! representation, suitable for comparing duplicate request identities.

use super::{Domain, Opcode, Reader, RequestId, VERSION, Writer, valid_token};
use crate::model::{
    flat::{BlockRef, SlotPosition, SlotVersion},
    identity::PendId,
    nest::{MemoGeneration, MemoPosition},
    view::ViewId,
};

/// A live-grant argument plus a maintenance locator. The codec cannot establish
/// that the caller owns the bound branch/Task or that the record is Held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Maintenance {
    pub token: PendId,
    pub block: BlockRef,
}

impl Maintenance {
    fn valid(self) -> bool {
        valid_token(self.token.get())
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        out.word(self.token.get() as u64)?;
        out.bytes(&self.block.to_bytes())
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            token: input.token()?,
            block: BlockRef::from_bytes(input.bytes::<24>()?)?,
        })
    }
}

/// Logical source selection, never a physical ownership grant. Flat/Memo
/// versions are checked when Insert commits; they are not permanent residency
/// pins. Share coordinates use the original View origin, not a narrowed window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Flat {
        position: SlotPosition,
        expected: SlotVersion,
    },
    Share {
        token: PendId,
        view: ViewId,
        page: usize,
        count: usize,
    },
    Memo {
        position: MemoPosition,
        expected: MemoGeneration,
    },
}

impl Source {
    fn valid(self) -> bool {
        match self {
            Self::Flat { .. } | Self::Memo { .. } => true,
            Self::Share {
                token,
                view,
                page,
                count,
            } => {
                valid_token(token.get())
                    && valid_token(view.get())
                    && crate::model::cell::pages(page, count).is_some()
            }
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::Flat { position, expected } => {
                out.word(0)?;
                write_slot(out, position, expected)
            }
            Self::Share {
                token,
                view,
                page,
                count,
            } => {
                out.word(1)?;
                for word in [token.get(), view.get(), page, count] {
                    out.word(word as u64)?;
                }
                Some(())
            }
            Self::Memo { position, expected } => {
                out.word(2)?;
                write_memo(out, position, expected)
            }
        }
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(match input.word()? {
            0 => {
                let (position, expected) = read_slot(input)?;
                Self::Flat { position, expected }
            }
            1 => Self::Share {
                token: input.token()?,
                view: ViewId::new(input.size()?),
                page: input.size()?,
                count: input.size()?,
            },
            2 => {
                let (position, expected) = read_memo(input)?;
                Self::Memo { position, expected }
            }
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Query tag 3; resolves only the actual caller's retained result. No live
    /// resource token is necessary after an accepted deletion or revocation.
    InspectResult {
        domain: Domain,
        request: RequestId,
    },
    Occupy {
        slot: SlotPosition,
        expected: SlotVersion,
    },
    Vacate {
        slot: SlotPosition,
        expected: SlotVersion,
        maintenance: Maintenance,
    },
    /// A fixed range; recovery names an existing orphaned record in this Flat.
    Block {
        start: SlotPosition,
        count: usize,
        recover: Option<BlockRef>,
    },
    Allow {
        maintenance: Maintenance,
    },
    Push {
        local: SlotPosition,
        local_version: SlotVersion,
        remote: SlotPosition,
        remote_version: SlotVersion,
        maintenance: Option<Maintenance>,
    },
    Pull {
        local: SlotPosition,
        local_version: SlotVersion,
        remote: SlotPosition,
        remote_version: SlotVersion,
        maintenance: Option<Maintenance>,
    },
    Insert {
        target: MemoPosition,
        expected: MemoGeneration,
        source: Source,
    },
    Remove {
        memo: MemoPosition,
        expected: MemoGeneration,
    },
    Publish {
        memo: MemoPosition,
        expected: MemoGeneration,
        maintenance: Option<Maintenance>,
    },
    Retract {
        memo: MemoPosition,
        expected: MemoGeneration,
    },
    Protect {
        memo: MemoPosition,
        expected: MemoGeneration,
        access: u64,
    },
}

impl Action {
    pub const fn opcode(self) -> Opcode {
        match self {
            Self::InspectResult { domain, .. } => Opcode::Inspect(domain),
            Self::Occupy { .. } => Opcode::Occupy,
            Self::Vacate { .. } => Opcode::Vacate,
            Self::Block { .. } => Opcode::Block,
            Self::Allow { .. } => Opcode::Allow,
            Self::Push { .. } => Opcode::Push,
            Self::Pull { .. } => Opcode::Pull,
            Self::Insert { .. } => Opcode::Insert,
            Self::Remove { .. } => Opcode::Remove,
            Self::Publish { .. } => Opcode::Publish,
            Self::Retract { .. } => Opcode::Retract,
            Self::Protect { .. } => Opcode::Protect,
        }
    }

    fn valid(self) -> bool {
        match self {
            Self::Vacate { maintenance, .. } | Self::Allow { maintenance } => maintenance.valid(),
            Self::Block {
                start,
                count,
                recover,
            } => {
                crate::model::cell::pages(start.page(), count).is_some()
                    && recover.is_none_or(|block| block.flat() == start.flat())
            }
            Self::Push {
                local,
                remote,
                maintenance,
                ..
            }
            | Self::Pull {
                local,
                remote,
                maintenance,
                ..
            } => {
                // Different grants to the same position do not create a second holding.
                (local.flat() != remote.flat() || local.page() != remote.page())
                    && maintenance.is_none_or(Maintenance::valid)
            }
            Self::Publish { maintenance, .. } => maintenance.is_none_or(Maintenance::valid),
            Self::Insert { source, .. } => source.valid(),
            Self::Protect { access, .. } => {
                access & !14 == 0 && access & 10 != 0 && (access & 4 == 0 || access & 2 != 0)
            }
            _ => true,
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::InspectResult { request, .. } => {
                out.word(3)?;
                out.word(request.get())
            }
            Self::Occupy { slot, expected } => write_slot(out, slot, expected),
            Self::Vacate {
                slot,
                expected,
                maintenance,
            } => {
                write_slot(out, slot, expected)?;
                maintenance.write(out)
            }
            Self::Block {
                start,
                count,
                recover,
            } => {
                out.bytes(&start.to_bytes())?;
                out.word(count as u64)?;
                out.word(u64::from(recover.is_some()))?;
                if let Some(block) = recover {
                    out.bytes(&block.to_bytes())?;
                }
                Some(())
            }
            Self::Allow { maintenance } => maintenance.write(out),
            Self::Push {
                local,
                local_version,
                remote,
                remote_version,
                maintenance,
            }
            | Self::Pull {
                local,
                local_version,
                remote,
                remote_version,
                maintenance,
            } => {
                write_slot(out, local, local_version)?;
                write_slot(out, remote, remote_version)?;
                write_maintenance(out, maintenance)
            }
            Self::Insert {
                target,
                expected,
                source,
            } => {
                write_memo(out, target, expected)?;
                source.write(out)
            }
            Self::Remove { memo, expected } | Self::Retract { memo, expected } => {
                write_memo(out, memo, expected)
            }
            Self::Publish {
                memo,
                expected,
                maintenance,
            } => {
                write_memo(out, memo, expected)?;
                write_maintenance(out, maintenance)
            }
            Self::Protect {
                memo,
                expected,
                access,
            } => {
                write_memo(out, memo, expected)?;
                out.word(access)
            }
        }
    }

    fn read(opcode: Opcode, input: &mut Reader<'_>) -> Option<Self> {
        Some(match opcode {
            Opcode::Inspect(domain) => {
                if input.word()? != 3 {
                    return None;
                }
                Self::InspectResult {
                    domain,
                    request: RequestId::new(input.word()?)?,
                }
            }
            Opcode::Occupy => {
                let (slot, expected) = read_slot(input)?;
                Self::Occupy { slot, expected }
            }
            Opcode::Vacate => {
                let (slot, expected) = read_slot(input)?;
                Self::Vacate {
                    slot,
                    expected,
                    maintenance: Maintenance::read(input)?,
                }
            }
            Opcode::Block => {
                let start = SlotPosition::from_bytes(input.bytes::<24>()?)?;
                let count = input.size()?;
                let recover = match input.word()? {
                    0 => None,
                    1 => Some(BlockRef::from_bytes(input.bytes::<24>()?)?),
                    _ => return None,
                };
                Self::Block {
                    start,
                    count,
                    recover,
                }
            }
            Opcode::Allow => Self::Allow {
                maintenance: Maintenance::read(input)?,
            },
            Opcode::Push | Opcode::Pull => {
                let (local, local_version) = read_slot(input)?;
                let (remote, remote_version) = read_slot(input)?;
                let maintenance = read_maintenance(input)?;
                if opcode == Opcode::Push {
                    Self::Push {
                        local,
                        local_version,
                        remote,
                        remote_version,
                        maintenance,
                    }
                } else {
                    Self::Pull {
                        local,
                        local_version,
                        remote,
                        remote_version,
                        maintenance,
                    }
                }
            }
            Opcode::Insert => {
                let (target, expected) = read_memo(input)?;
                Self::Insert {
                    target,
                    expected,
                    source: Source::read(input)?,
                }
            }
            Opcode::Remove => {
                let (memo, expected) = read_memo(input)?;
                Self::Remove { memo, expected }
            }
            Opcode::Publish => {
                let (memo, expected) = read_memo(input)?;
                Self::Publish {
                    memo,
                    expected,
                    maintenance: read_maintenance(input)?,
                }
            }
            Opcode::Retract => {
                let (memo, expected) = read_memo(input)?;
                Self::Retract { memo, expected }
            }
            Opcode::Protect => {
                let (memo, expected) = read_memo(input)?;
                Self::Protect {
                    memo,
                    expected,
                    access: input.word()?,
                }
            }
        })
    }
}

/// Exactly 256 bytes: [version, domain, operation, request_id], action payload,
/// then reserved zero words. An inspection has request_id=0; every mutation has
/// a nonzero caller-chosen ID. No output pointers participate in request identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    id: Option<RequestId>,
    action: Action,
}

impl Request {
    pub const WIDTH: usize = 256;

    pub fn new(id: Option<RequestId>, action: Action) -> Option<Self> {
        let inspection = matches!(action, Action::InspectResult { .. });
        (id.is_none() == inspection && action.valid()).then_some(Self { id, action })
    }

    pub const fn id(self) -> Option<RequestId> {
        self.id
    }
    pub const fn action(self) -> Action {
        self.action
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        self.write(&mut Writer(&mut bytes))
            .expect("bounded memory request");
        bytes
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        let opcode = self.action.opcode();
        for word in [
            VERSION,
            opcode.domain() as u64,
            opcode.slot(),
            self.id.map_or(0, RequestId::get),
        ] {
            out.word(word)?;
        }
        self.action.write(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let mut input = Reader(bytes);
        if input.word()? != VERSION {
            return None;
        }
        let opcode = Opcode::of(input.word()?, input.word()?)?;
        let id = RequestId::new(input.word()?);
        let action = Action::read(opcode, &mut input)?;
        input.finish()?;
        Self::new(id, action)
    }

    /// Entry must match the external trap number to the snapshotted descriptor.
    /// A structurally valid payload cannot select another operation/domain.
    pub fn from_call(number: u64, bytes: &[u8]) -> Option<Self> {
        let opcode = Opcode::from_call_number(number)?;
        let request = Self::from_bytes(bytes)?;
        (request.action.opcode() == opcode).then_some(request)
    }
}

fn write_slot(out: &mut Writer<'_>, position: SlotPosition, version: SlotVersion) -> Option<()> {
    out.bytes(&position.to_bytes())?;
    out.word(version.get())
}

fn read_slot(input: &mut Reader<'_>) -> Option<(SlotPosition, SlotVersion)> {
    Some((
        SlotPosition::from_bytes(input.bytes::<24>()?)?,
        SlotVersion::new(input.word()?)?,
    ))
}

fn write_memo(
    out: &mut Writer<'_>,
    position: MemoPosition,
    generation: MemoGeneration,
) -> Option<()> {
    out.bytes(&position.to_bytes())?;
    out.word(generation.get())
}

fn read_memo(input: &mut Reader<'_>) -> Option<(MemoPosition, MemoGeneration)> {
    Some((
        MemoPosition::from_bytes(input.bytes::<32>()?)?,
        MemoGeneration::new(input.word()?)?,
    ))
}

fn write_maintenance(out: &mut Writer<'_>, maintenance: Option<Maintenance>) -> Option<()> {
    out.word(u64::from(maintenance.is_some()))?;
    if let Some(maintenance) = maintenance {
        maintenance.write(out)?;
    }
    Some(())
}

fn read_maintenance(input: &mut Reader<'_>) -> Option<Option<Maintenance>> {
    Some(match input.word()? {
        0 => None,
        1 => Some(Maintenance::read(input)?),
        _ => return None,
    })
}
