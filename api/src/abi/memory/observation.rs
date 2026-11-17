//! Explicit object/position observations. Result queries remain in Request and
//! Reply: an observation has no request identity and cannot be a commit receipt.

use super::{
    Action, Domain, Maintenance, Opcode, Reader, Request, VERSION, Writer, flat as domain_flat,
    nest as domain_nest, valid_token,
};
use crate::address::VirtAddr;
use crate::model::{
    cell::CellLevel,
    flat::{
        BlockPhase, BlockRef, BlockStatus, FlatId, FlatPhase, FlatStatus, FlatVersion, SlotPhase,
        SlotPosition, SlotStatus, SlotVersion,
    },
    identity::{PendId, TaskId},
    nest::SpanId,
    nest::{
        FaultAccess, FaultCause, FaultEpoch, FaultEvent, FaultPhase, FaultSequence, FaultStatus,
        MemoGeneration, MemoPhase, MemoPosition, MemoStatus, NestId, NestInfo, NestKind, NestPhase,
        NestStatus, NestVersion, SpanPhase, SpanStatus, SpanVersion,
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
        span: SpanId,
    },
    /// Observe the next retained event without consuming it. Only the actual
    /// registered handler may read events; timeout never resolves a fault.
    FaultEvent {
        token: PendId,
        nest: NestId,
        span: SpanId,
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
    Span {
        token: PendId,
        nest: NestId,
        span: SpanId,
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
            Self::Block(_) | Self::Span { .. } => 2,
            Self::FaultRoute { .. } => 4,
            Self::FaultEvent { .. } => 5,
        }
    }
    pub(crate) fn valid(self) -> bool {
        match self {
            Self::Flat { token, .. } | Self::Nest { token, .. } => valid_token(token.get()),
            Self::Block(h) => valid_token(h.token.get()),
            Self::Span { token, span, .. } | Self::FaultRoute { token, span, .. } => {
                valid_token(token.get()) && valid_token(span.get())
            }
            Self::FaultEvent {
                token, span, wait, ..
            } => valid_token(token.get()) && valid_token(span.get()) && wait != Wait::Infinite,
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
            Self::Span { token, nest, span } | Self::FaultRoute { token, nest, span } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(span.get() as u64)
            }
            Self::FaultEvent {
                token,
                nest,
                span,
                epoch,
                after,
                wait,
            } => {
                for word in [
                    token.get() as u64,
                    nest.get() as u64,
                    span.get() as u64,
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
            (Domain::Nest, 1) => Self::Memo(MemoPosition::from_bytes(input.bytes::<16>()?)?),
            (Domain::Nest, 2) => Self::Span {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
            },
            (Domain::Nest, 4) => Self::FaultRoute {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
            },
            (Domain::Nest, 5) => Self::FaultEvent {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
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
    Span(SpanStatus),
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
            Self::Block(_) | Self::Span(_) => 2,
            Self::FaultRoute(_) => 4,
            Self::FaultEvent(_) => 5,
        }
    }
    fn valid(self) -> bool {
        match self {
            Self::Flat(_) => {
                snapshot::flat::Snapshot::try_from(self).is_ok_and(snapshot::flat::Snapshot::valid)
            }
            Self::Slot(_) | Self::Block(_) => {
                snapshot::flat::Snapshot::try_from(self).is_ok_and(snapshot::flat::Snapshot::valid)
            }
            _ => {
                snapshot::nest::Snapshot::try_from(self).is_ok_and(snapshot::nest::Snapshot::valid)
            }
        }
    }
    fn matches(self, query: Query) -> bool {
        match query.domain() {
            Domain::Flat => snapshot::flat::Snapshot::try_from(self).is_ok_and(|value| {
                snapshot::flat::Snapshot::matches(
                    value,
                    flat::Query::try_from(query).expect("flat query"),
                )
            }),
            Domain::Nest => snapshot::nest::Snapshot::try_from(self).is_ok_and(|value| {
                snapshot::nest::Snapshot::matches(
                    value,
                    nest::Query::try_from(query).expect("nest query"),
                )
            }),
        }
    }
    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::FaultRoute(s) => words(
                out,
                &[
                    s.nest.get() as u64,
                    s.span.get() as u64,
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
                    s.span.get() as u64,
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
                        s.range.map_or(0, |range| range.get() as u64),
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
            Self::Span(s) => words(
                out,
                &[
                    s.nest.get() as u64,
                    s.span.get() as u64,
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
                span: input.span()?,
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
                let span = input.span()?;
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
                    span,
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
                let range = match input.size()? {
                    0 => None,
                    range => Some(crate::model::identity::PendId::mint(range)),
                };
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
                    range,
                    coordinate,
                    generation,
                    phase,
                    source,
                    page: input.word()?,
                    access: input.word()?,
                    level: CellLevel::new(input.size()?)?,
                })
            }

            (Domain::Nest, 2) => Self::Span(SpanStatus {
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                version: SpanVersion::new(input.word()?)?,
                offset: input.size()?,
                count: input.size()?,
                address: VirtAddr::new(input.size()?),
                level: CellLevel::new(input.size()?)?,
                access: input.word()?,
                phase: SpanPhase::from_tag(input.word()?)?,
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
                match self.0.domain() {
                    Domain::Flat => Opcode::Flat(domain_flat::Opcode::Inspect),
                    Domain::Nest => Opcode::Nest(domain_nest::Opcode::Inspect),
                }
                .slot(),
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

/// Flat-domain observations.
pub mod flat {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Query {
        Flat { token: PendId, flat: FlatId },
        Slot(SlotPosition),
        Block(Maintenance),
    }

    impl From<Query> for super::Query {
        fn from(query: Query) -> Self {
            match query {
                Query::Flat { token, flat } => Self::Flat { token, flat },
                Query::Slot(slot) => Self::Slot(slot),
                Query::Block(block) => Self::Block(block),
            }
        }
    }

    impl TryFrom<super::Query> for Query {
        type Error = ();

        fn try_from(query: super::Query) -> Result<Self, Self::Error> {
            Ok(match query {
                super::Query::Flat { token, flat } => Self::Flat { token, flat },
                super::Query::Slot(slot) => Self::Slot(slot),
                super::Query::Block(block) => Self::Block(block),
                _ => return Err(()),
            })
        }
    }
}

/// Nest-domain observations. The range is still named by the internal Span in
/// this transitional type; the Pend-bound range replaces it in the next slice.
pub mod nest {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Query {
        FaultRoute {
            token: PendId,
            nest: NestId,
            span: SpanId,
        },
        FaultEvent {
            token: PendId,
            nest: NestId,
            span: SpanId,
            epoch: FaultEpoch,
            after: u64,
            wait: Wait,
        },
        Nest {
            token: PendId,
            nest: NestId,
        },
        Memo(MemoPosition),
        Span {
            token: PendId,
            nest: NestId,
            span: SpanId,
        },
    }

    impl From<Query> for super::Query {
        fn from(query: Query) -> Self {
            match query {
                Query::FaultRoute { token, nest, span } => Self::FaultRoute { token, nest, span },
                Query::FaultEvent {
                    token,
                    nest,
                    span,
                    epoch,
                    after,
                    wait,
                } => Self::FaultEvent {
                    token,
                    nest,
                    span,
                    epoch,
                    after,
                    wait,
                },
                Query::Nest { token, nest } => Self::Nest { token, nest },
                Query::Memo(memo) => Self::Memo(memo),
                Query::Span { token, nest, span } => Self::Span { token, nest, span },
            }
        }
    }

    impl TryFrom<super::Query> for Query {
        type Error = ();

        fn try_from(query: super::Query) -> Result<Self, Self::Error> {
            Ok(match query {
                super::Query::FaultRoute { token, nest, span } => {
                    Self::FaultRoute { token, nest, span }
                }
                super::Query::FaultEvent {
                    token,
                    nest,
                    span,
                    epoch,
                    after,
                    wait,
                } => Self::FaultEvent {
                    token,
                    nest,
                    span,
                    epoch,
                    after,
                    wait,
                },
                super::Query::Nest { token, nest } => Self::Nest { token, nest },
                super::Query::Memo(memo) => Self::Memo(memo),
                super::Query::Span { token, nest, span } => Self::Span { token, nest, span },
                _ => return Err(()),
            })
        }
    }
}

/// Flat-domain snapshots.
pub mod snapshot {
    pub mod flat {
        use super::super::*;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Snapshot {
            Flat(FlatStatus),
            Slot(SlotStatus),
            Block(BlockStatus),
        }

        impl Snapshot {
            pub(crate) fn valid(self) -> bool {
                match self {
                    Self::Flat(s) => s.valid(),
                    Self::Slot(s) => s.valid(),
                    Self::Block(s) => s.valid(),
                }
            }

            pub(crate) fn matches(self, query: super::super::flat::Query) -> bool {
                match (self, query) {
                    (Self::Flat(s), super::super::flat::Query::Flat { flat, .. }) => s.id == flat,
                    (Self::Slot(s), super::super::flat::Query::Slot(p)) => {
                        s.flat == p.flat() && s.coordinate == p.page()
                    }
                    (Self::Block(s), super::super::flat::Query::Block(h)) => s.block == h.block,
                    _ => false,
                }
            }
        }

        impl From<Snapshot> for super::super::Snapshot {
            fn from(value: Snapshot) -> Self {
                match value {
                    Snapshot::Flat(status) => Self::Flat(status),
                    Snapshot::Slot(status) => Self::Slot(status),
                    Snapshot::Block(status) => Self::Block(status),
                }
            }
        }

        impl TryFrom<super::super::Snapshot> for Snapshot {
            type Error = ();

            fn try_from(value: super::super::Snapshot) -> Result<Self, Self::Error> {
                Ok(match value {
                    super::super::Snapshot::Flat(status) => Self::Flat(status),
                    super::super::Snapshot::Slot(status) => Self::Slot(status),
                    super::super::Snapshot::Block(status) => Self::Block(status),
                    _ => return Err(()),
                })
            }
        }
    }

    /// Nest-domain snapshots.
    pub mod nest {
        use super::super::*;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Snapshot {
            FaultRoute(FaultStatus),
            FaultEvent(FaultEvent),
            Nest(NestStatus),
            Memo(MemoStatus),
            Span(SpanStatus),
        }

        impl Snapshot {
            pub(crate) fn valid(self) -> bool {
                match self {
                    Self::FaultRoute(s) => s.valid(),
                    Self::FaultEvent(s) => s.valid(),
                    Self::Nest(_) => true,
                    Self::Memo(s) => s.valid(),
                    Self::Span(s) => s.valid(),
                }
            }

            pub(crate) fn matches(self, query: super::super::nest::Query) -> bool {
                use super::super::nest::Query;
                match (self, query) {
                    (Self::FaultRoute(s), Query::FaultRoute { nest, span, .. }) => {
                        s.nest == nest && s.span == span
                    }
                    (
                        Self::FaultEvent(s),
                        Query::FaultEvent {
                            nest,
                            span,
                            epoch,
                            after,
                            ..
                        },
                    ) => {
                        s.nest == nest
                            && s.span == span
                            && s.epoch == epoch
                            && s.sequence.get() > after
                    }
                    (Self::Nest(s), Query::Nest { nest, .. }) => s.info.id == nest,
                    (Self::Memo(s), Query::Memo(p)) => {
                        s.range() == Some(p.range()) && s.coordinate == p.page()
                    }
                    (Self::Span(s), Query::Span { nest, span, .. }) => {
                        s.nest == nest && s.span == span
                    }
                    _ => false,
                }
            }
        }

        impl From<Snapshot> for super::super::Snapshot {
            fn from(value: Snapshot) -> Self {
                match value {
                    Snapshot::FaultRoute(status) => Self::FaultRoute(status),
                    Snapshot::FaultEvent(event) => Self::FaultEvent(event),
                    Snapshot::Nest(status) => Self::Nest(status),
                    Snapshot::Memo(status) => Self::Memo(status),
                    Snapshot::Span(status) => Self::Span(status),
                }
            }
        }

        impl TryFrom<super::super::Snapshot> for Snapshot {
            type Error = ();

            fn try_from(value: super::super::Snapshot) -> Result<Self, Self::Error> {
                Ok(match value {
                    super::super::Snapshot::FaultRoute(status) => Self::FaultRoute(status),
                    super::super::Snapshot::FaultEvent(event) => Self::FaultEvent(event),
                    super::super::Snapshot::Nest(status) => Self::Nest(status),
                    super::super::Snapshot::Memo(status) => Self::Memo(status),
                    super::super::Snapshot::Span(status) => Self::Span(status),
                    _ => return Err(()),
                })
            }
        }
    }
}
