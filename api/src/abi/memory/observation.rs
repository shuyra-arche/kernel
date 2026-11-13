//! Explicit object/position observations. Result queries remain in Request and
//! Reply: an observation has no request identity and cannot be a commit receipt.

use super::{Action, Domain, Maintenance, Opcode, Reader, Request, VERSION, Writer, valid_token};
use crate::address::VirtAddr;
use crate::model::{
    cell::{CellLevel, MappingId},
    flat::{
        BlockPhase, BlockRef, BlockStatus, FlatId, FlatPhase, FlatStatus, FlatVersion, SlotPhase,
        SlotPosition, SlotStatus, SlotVersion,
    },
    identity::{PendId, TaskId},
    nest::{
        FaultAccess, FaultCause, FaultEpoch, FaultEvent, FaultPhase, FaultSequence, FaultStatus,
        MappingPhase, MappingStatus, MappingVersion, MemoGeneration, MemoPhase, MemoPosition,
        MemoStatus, NestId, NestInfo, NestKind, NestPhase, NestStatus, NestVersion,
    },
    wait::Wait,
};

/// State queries do not require the version being observed. Event reads name
/// an existing route epoch and cursor, never an expected residency version. The
/// token is resolved afresh under its live scope at observation/delivery time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    FaultRoute {
        token: PendId,
        nest: NestId,
        mapping: MappingId,
    },
    /// Observe the next retained event without consuming it. Only the actual
    /// registered handler may read events; timeout never resolves a fault.
    FaultEvent {
        token: PendId,
        nest: NestId,
        mapping: MappingId,
        epoch: FaultEpoch,
        after: u64,
        wait: Wait,
    },
    Flat {
        token: PendId,
        flat: FlatId,
    },
    Slot(SlotPosition),
    Block(Maintenance),
    Nest {
        token: PendId,
        nest: NestId,
    },
    Memo(MemoPosition),
    Mapping {
        token: PendId,
        nest: NestId,
        mapping: MappingId,
    },
}

impl Query {
    pub const fn domain(self) -> Domain {
        match self {
            Self::Flat { .. } | Self::Slot(_) | Self::Block(_) => Domain::Flat,
            _ => Domain::Nest,
        }
    }
    pub const fn tag(self) -> u64 {
        match self {
            Self::Flat { .. } | Self::Nest { .. } => 0,
            Self::Slot(_) | Self::Memo(_) => 1,
            Self::Block(_) | Self::Mapping { .. } => 2,
            Self::FaultRoute { .. } => 4,
            Self::FaultEvent { .. } => 5,
        }
    }
    pub(super) fn valid(self) -> bool {
        match self {
            Self::Flat { token, .. } | Self::Nest { token, .. } => valid_token(token.get()),
            Self::Block(h) => valid_token(h.token.get()),
            Self::Mapping { token, mapping, .. } | Self::FaultRoute { token, mapping, .. } => {
                valid_token(token.get()) && valid_token(mapping.get())
            }
            Self::FaultEvent {
                token,
                mapping,
                wait,
                ..
            } => valid_token(token.get()) && valid_token(mapping.get()) && wait != Wait::Infinite,
            _ => true,
        }
    }
    pub(super) fn write(self, out: &mut Writer<'_>) -> Option<()> {
        out.word(self.tag())?;
        match self {
            Self::Flat { token, flat } => {
                out.word(token.get() as u64)?;
                out.word(flat.get() as u64)
            }
            Self::Slot(slot) => out.bytes(&slot.to_bytes()),
            Self::Block(h) => {
                out.word(h.token.get() as u64)?;
                out.bytes(&h.block.to_bytes())
            }
            Self::Nest { token, nest } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)
            }
            Self::Memo(memo) => out.bytes(&memo.to_bytes()),
            Self::Mapping {
                token,
                nest,
                mapping,
            }
            | Self::FaultRoute {
                token,
                nest,
                mapping,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(mapping.get() as u64)
            }
            Self::FaultEvent {
                token,
                nest,
                mapping,
                epoch,
                after,
                wait,
            } => {
                for word in [
                    token.get() as u64,
                    nest.get() as u64,
                    mapping.get() as u64,
                    epoch.get(),
                    after,
                ] {
                    out.word(word)?;
                }
                match wait {
                    Wait::Poll => {
                        out.word(0)?;
                        out.word(0)
                    }
                    Wait::At(at) => {
                        out.word(1)?;
                        out.word(at)
                    }
                    Wait::Infinite => None,
                }
            }
        }
    }
    pub(super) fn read(domain: Domain, tag: u64, input: &mut Reader<'_>) -> Option<Self> {
        Some(match (domain, tag) {
            (Domain::Flat, 0) => Self::Flat {
                token: input.token()?,
                flat: FlatId::new(input.size()?)?,
            },
            (Domain::Flat, 1) => Self::Slot(SlotPosition::from_bytes(input.bytes::<24>()?)?),
            (Domain::Flat, 2) => Self::Block(Maintenance {
                token: input.token()?,
                block: BlockRef::from_bytes(input.bytes::<24>()?)?,
            }),
            (Domain::Nest, 0) => Self::Nest {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
            },
            (Domain::Nest, 1) => Self::Memo(MemoPosition::from_bytes(input.bytes::<32>()?)?),
            (Domain::Nest, 2) => Self::Mapping {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
            },
            (Domain::Nest, 4) => Self::FaultRoute {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
            },
            (Domain::Nest, 5) => Self::FaultEvent {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                epoch: FaultEpoch::new(input.word()?)?,
                after: input.word()?,
                wait: match (input.word()?, input.word()?) {
                    (0, 0) => Wait::Poll,
                    (1, at) => Wait::At(at),
                    _ => return None,
                },
            },
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snapshot {
    FaultRoute(FaultStatus),
    FaultEvent(FaultEvent),
    Flat(FlatStatus),
    Slot(SlotStatus),
    Block(BlockStatus),
    Nest(NestStatus),
    Memo(MemoStatus),
    Mapping(MappingStatus),
}

impl Snapshot {
    fn domain(self) -> Domain {
        match self {
            Self::Flat(_) | Self::Slot(_) | Self::Block(_) => Domain::Flat,
            _ => Domain::Nest,
        }
    }
    fn tag(self) -> u64 {
        match self {
            Self::Flat(_) | Self::Nest(_) => 0,
            Self::Slot(_) | Self::Memo(_) => 1,
            Self::Block(_) | Self::Mapping(_) => 2,
            Self::FaultRoute(_) => 4,
            Self::FaultEvent(_) => 5,
        }
    }
    fn valid(self) -> bool {
        match self {
            Self::Flat(s) => s.valid(),
            Self::Slot(s) => s.valid(),
            Self::Block(s) => s.valid(),
            Self::Nest(_) => true,
            Self::Memo(s) => s.valid(),
            Self::Mapping(s) => s.valid(),
            Self::FaultRoute(s) => s.valid(),
            Self::FaultEvent(s) => s.valid(),
        }
    }
    fn matches(self, query: Query) -> bool {
        match (self, query) {
            (Self::FaultRoute(s), Query::FaultRoute { nest, mapping, .. }) => {
                s.nest == nest && s.mapping == mapping
            }
            (
                Self::FaultEvent(s),
                Query::FaultEvent {
                    nest,
                    mapping,
                    epoch,
                    after,
                    ..
                },
            ) => {
                s.nest == nest
                    && s.mapping == mapping
                    && s.epoch == epoch
                    && s.sequence.get() > after
            }
            (Self::Flat(s), Query::Flat { flat, .. }) => s.id == flat,
            (Self::Slot(s), Query::Slot(p)) => s.flat == p.flat() && s.coordinate == p.page(),
            (Self::Block(s), Query::Block(h)) => s.block == h.block,
            (Self::Nest(s), Query::Nest { nest, .. }) => s.info.id == nest,
            (Self::Memo(s), Query::Memo(p)) => {
                s.nest == p.nest() && s.mapping == p.mapping() && s.coordinate == p.page()
            }
            (Self::Mapping(s), Query::Mapping { nest, mapping, .. }) => {
                s.nest == nest && s.mapping == mapping
            }
            _ => false,
        }
    }
    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::FaultRoute(s) => words(
                out,
                &[
                    s.nest.get() as u64,
                    s.mapping.get() as u64,
                    s.epoch.get(),
                    s.phase as u64,
                    s.handler.map_or(0, |task| task.get()) as u64,
                    s.timeout_ns,
                    s.last_sequence,
                ],
            ),
            Self::FaultEvent(s) => words(
                out,
                &[
                    s.nest.get() as u64,
                    s.mapping.get() as u64,
                    s.epoch.get(),
                    s.sequence.get(),
                    s.memo_page as u64,
                    s.generation.get(),
                    s.flat.get() as u64,
                    s.slot_page as u64,
                    s.version.get(),
                    s.cause as u64 | ((s.access as u64) << 8),
                ],
            ),
            Self::Flat(s) => words(
                out,
                &[
                    s.id.get() as u64,
                    s.version.get(),
                    s.offset as u64,
                    s.count as u64,
                    s.phase as u64,
                ],
            ),
            Self::Slot(s) => words(
                out,
                &[
                    s.flat.get() as u64,
                    s.coordinate as u64,
                    s.version.get(),
                    s.level.get() as u64,
                    s.phase as u64,
                    s.page,
                    s.obligations,
                    u64::from(s.blocked),
                ],
            ),
            Self::Block(s) => {
                out.bytes(&s.block.to_bytes())?;
                words(
                    out,
                    &[
                        s.phase as u64,
                        s.offset as u64,
                        s.count as u64,
                        s.actor.get() as u64,
                        s.ordinary,
                        s.maintenance,
                        u64::from(s.failed),
                    ],
                )
            }
            Self::Nest(s) => words(
                out,
                &[
                    s.info.id.get() as u64,
                    s.version.get(),
                    s.info.kind as u64,
                    s.info.phase as u64,
                    s.info.fault_sequence,
                    s.info.fault_cause,
                    s.info.fault_address,
                    s.info.fault_requester,
                ],
            ),
            Self::Memo(s) => {
                let (source, page) = s.source.map_or((0, 0), |(flat, page)| (flat.get(), page));
                words(
                    out,
                    &[
                        s.nest.get() as u64,
                        s.mapping.get() as u64,
                        s.coordinate as u64,
                        s.generation.get(),
                        s.phase as u64,
                        source as u64,
                        page as u64,
                        s.page,
                        s.access,
                        s.level.get() as u64,
                    ],
                )
            }
            Self::Mapping(s) => words(
                out,
                &[
                    s.nest.get() as u64,
                    s.mapping.get() as u64,
                    s.version.get(),
                    s.offset as u64,
                    s.count as u64,
                    s.address.get() as u64,
                    s.level.get() as u64,
                    s.access,
                    s.phase as u64,
                ],
            ),
        }
    }
    fn read(domain: Domain, tag: u64, input: &mut Reader<'_>) -> Option<Self> {
        Some(match (domain, tag) {
            (Domain::Nest, 4) => Self::FaultRoute(FaultStatus {
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                epoch: FaultEpoch::new(input.word()?)?,
                phase: FaultPhase::from_tag(input.word()?)?,
                handler: match input.size()? {
                    0 => None,
                    task => Some(TaskId::new(task)),
                },
                timeout_ns: input.word()?,
                last_sequence: input.word()?,
            }),
            (Domain::Nest, 5) => {
                let nest = NestId::new(input.size()?)?;
                let mapping = input.mapping()?;
                let epoch = FaultEpoch::new(input.word()?)?;
                let sequence = FaultSequence::new(input.word()?)?;
                let memo_page = input.size()?;
                let generation = MemoGeneration::new(input.word()?)?;
                let flat = FlatId::new(input.size()?)?;
                let slot_page = input.size()?;
                let version = SlotVersion::new(input.word()?)?;
                let flags = input.word()?;
                if flags >> 16 != 0 {
                    return None;
                }
                Self::FaultEvent(FaultEvent {
                    nest,
                    mapping,
                    epoch,
                    sequence,
                    memo_page,
                    generation,
                    flat,
                    slot_page,
                    version,
                    cause: FaultCause::from_tag(flags & 255)?,
                    access: FaultAccess::from_tag(flags >> 8)?,
                })
            }
            (Domain::Flat, 0) => Self::Flat(FlatStatus {
                id: FlatId::new(input.size()?)?,
                version: FlatVersion::new(input.word()?)?,
                offset: input.size()?,
                count: input.size()?,
                phase: FlatPhase::from_tag(input.word()?)?,
            }),
            (Domain::Flat, 1) => Self::Slot(SlotStatus {
                flat: FlatId::new(input.size()?)?,
                coordinate: input.size()?,
                version: SlotVersion::new(input.word()?)?,
                level: CellLevel::new(input.size()?)?,
                phase: SlotPhase::from_tag(input.word()?)?,
                page: input.word()?,
                obligations: input.word()?,
                blocked: boolean(input)?,
            }),
            (Domain::Flat, 2) => Self::Block(BlockStatus {
                block: BlockRef::from_bytes(input.bytes::<24>()?)?,
                phase: BlockPhase::from_tag(input.word()?)?,
                offset: input.size()?,
                count: input.size()?,
                actor: TaskId::new(input.size()?),
                ordinary: input.word()?,
                maintenance: input.word()?,
                failed: boolean(input)?,
            }),
            (Domain::Nest, 0) => {
                let id = NestId::new(input.size()?)?;
                let version = NestVersion::new(input.word()?)?;
                let kind = match input.word()? {
                    0 => NestKind::Internal,
                    1 => NestKind::External,
                    _ => return None,
                };
                let phase = match input.word()? {
                    0 => NestPhase::Unbound,
                    1 => NestPhase::Bound,
                    2 => NestPhase::Closing,
                    3 => NestPhase::Closed,
                    4 => NestPhase::Failed,
                    _ => return None,
                };
                Self::Nest(NestStatus {
                    version,
                    info: NestInfo {
                        id,
                        kind,
                        phase,
                        fault_sequence: input.word()?,
                        fault_cause: input.word()?,
                        fault_address: input.word()?,
                        fault_requester: input.word()?,
                    },
                })
            }
            (Domain::Nest, 1) => {
                let nest = NestId::new(input.size()?)?;
                let mapping = input.mapping()?;
                let coordinate = input.size()?;
                let generation = MemoGeneration::new(input.word()?)?;
                let phase = MemoPhase::from_tag(input.word()?)?;
                let source = input.size()?;
                let source_page = input.size()?;
                let source = match (source, source_page) {
                    (0, 0) => None,
                    (0, _) => return None,
                    (id, page) => Some((FlatId::new(id)?, page)),
                };
                Self::Memo(MemoStatus {
                    nest,
                    mapping,
                    coordinate,
                    generation,
                    phase,
                    source,
                    page: input.word()?,
                    access: input.word()?,
                    level: CellLevel::new(input.size()?)?,
                })
            }
            (Domain::Nest, 2) => Self::Mapping(MappingStatus {
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                version: MappingVersion::new(input.word()?)?,
                offset: input.size()?,
                count: input.size()?,
                address: VirtAddr::new(input.size()?),
                level: CellLevel::new(input.size()?)?,
                access: input.word()?,
                phase: MappingPhase::from_tag(input.word()?)?,
            }),
            _ => return None,
        })
    }
}

/// Exactly 128 bytes: [11, domain, Inspect, 0, query_tag, 0], then the typed
/// snapshot and a checked zero tail. A mutation Reply requires a nonzero ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation(Snapshot);

impl Observation {
    pub const WIDTH: usize = 128;
    pub fn new(snapshot: Snapshot) -> Option<Self> {
        snapshot.valid().then_some(Self(snapshot))
    }
    pub const fn snapshot(self) -> Snapshot {
        self.0
    }
    pub fn matches(self, request: Request) -> bool {
        matches!(request.action(), Action::Inspect(query) if self.0.matches(query))
    }
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        let mut out = Writer(&mut bytes);
        words(
            &mut out,
            &[
                VERSION,
                self.0.domain() as u64,
                Opcode::Inspect(self.0.domain()).slot(),
                0,
                self.0.tag(),
                0,
            ],
        )
        .expect("observation header");
        self.0.write(&mut out).expect("bounded memory observation");
        bytes
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let mut input = Reader(bytes);
        if input.word()? != VERSION {
            return None;
        }
        let domain = Domain::of(input.word()?)?;
        if input.word()? != 18 || input.word()? != 0 {
            return None;
        }
        let tag = input.word()?;
        if input.word()? != 0 {
            return None;
        }
        let snapshot = Snapshot::read(domain, tag, &mut input)?;
        input.finish()?;
        Self::new(snapshot)
    }
}

fn words(out: &mut Writer<'_>, words: &[u64]) -> Option<()> {
    for word in words {
        out.word(*word)?;
    }
    Some(())
}
fn boolean(input: &mut Reader<'_>) -> Option<bool> {
    match input.word()? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}
