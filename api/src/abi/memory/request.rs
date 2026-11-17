//! Bounded caller-memory requests for the settled Flat/Nest transitions. The
//! header names the domain and operation; unused bytes have one canonical zero
//! representation, suitable for comparing duplicate request identities.

use super::{
    Domain, Opcode, Query, Reader, RequestId, VERSION, Writer, flat as domain_flat,
    nest as domain_nest, valid_token,
};
use crate::model::{
    cell::CellLevel,
    flat::{BlockRef, FlatId, FlatVersion, Reshape, SlotPosition, SlotVersion},
    identity::PendId,
    nest::SpanId,
    nest::{
        CreateSpanRequest, FaultEpoch, FaultRoute, MemoGeneration, MemoPosition, NestId,
        NestVersion, SpanVersion,
    },
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
    /// Atomically install/replace or disable a Span-owned fault route. The
    /// provider binds the actual caller, never a caller-supplied handler Task.
    FaultRoute {
        token: PendId,
        nest: NestId,
        span: SpanId,
        expected: SpanVersion,
        epoch: FaultEpoch,
        route: Option<FaultRoute>,
    },
    CreateFlat {
        license: PendId,
        count: usize,
        level: CellLevel,
    },
    DeleteFlat {
        token: PendId,
        flat: FlatId,
        expected: FlatVersion,
    },
    CreateNest {
        license: PendId,
    },
    DeleteNest {
        token: PendId,
        nest: NestId,
        expected: NestVersion,
    },
    CreateSpan {
        token: PendId,
        nest: NestId,
        expected: NestVersion,
        request: CreateSpanRequest,
    },
    DeleteSpan {
        token: PendId,
        nest: NestId,
        span: SpanId,
        expected: SpanVersion,
    },
    /// Release a page-aligned subrange of a Span. The Span stays open and its
    /// revision advances; full close remains `DeleteSpan`.
    ReleaseRange {
        token: PendId,
        nest: NestId,
        span: SpanId,
        expected: SpanVersion,
        offset: usize,
        count: usize,
    },
    /// One parent to its 512 immediate children. The expected Flat revision
    /// covers every affected coordinate, not merely the first Slot's stamp.
    Split {
        start: SlotPosition,
        expected: FlatVersion,
        parent: CellLevel,
        maintenance: Maintenance,
    },
    /// The same parent-sized range, from 512 immediate children to one parent.
    Merge {
        start: SlotPosition,
        expected: FlatVersion,
        parent: CellLevel,
        maintenance: Maintenance,
    },
    Inspect(Query),
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
    /// A fixed range. `recover` resumes this Task/branch's Draining record, or
    /// takes over an orphaned record under a real ancestor with a new epoch.
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

const fn inspect_opcode(domain: Domain) -> Opcode {
    match domain {
        Domain::Flat => Opcode::Flat(domain_flat::Opcode::Inspect),
        Domain::Nest => Opcode::Nest(domain_nest::Opcode::Inspect),
    }
}

impl Action {
    pub const fn opcode(self) -> Opcode {
        match self {
            Self::FaultRoute { .. } => Opcode::Nest(domain_nest::Opcode::FaultRoute),
            Self::CreateFlat { .. } => Opcode::Flat(domain_flat::Opcode::Create),
            Self::DeleteFlat { .. } => Opcode::Flat(domain_flat::Opcode::Delete),
            Self::CreateNest { .. } => Opcode::Nest(domain_nest::Opcode::Create),
            Self::DeleteNest { .. } => Opcode::Nest(domain_nest::Opcode::Delete),
            Self::CreateSpan { .. } => Opcode::Nest(domain_nest::Opcode::Reserve),
            Self::DeleteSpan { .. } => Opcode::Nest(domain_nest::Opcode::Release),
            Self::ReleaseRange { .. } => Opcode::Nest(domain_nest::Opcode::ReleaseRange),
            Self::Split { .. } => Opcode::Flat(domain_flat::Opcode::Split),
            Self::Merge { .. } => Opcode::Flat(domain_flat::Opcode::Merge),
            Self::Inspect(query) => inspect_opcode(query.domain()),
            Self::InspectResult { domain, .. } => inspect_opcode(domain),
            Self::Occupy { .. } => Opcode::Flat(domain_flat::Opcode::Occupy),
            Self::Vacate { .. } => Opcode::Flat(domain_flat::Opcode::Vacate),
            Self::Block { .. } => Opcode::Flat(domain_flat::Opcode::Block),
            Self::Allow { .. } => Opcode::Flat(domain_flat::Opcode::Allow),
            Self::Push { .. } => Opcode::Flat(domain_flat::Opcode::Push),
            Self::Pull { .. } => Opcode::Flat(domain_flat::Opcode::Pull),
            Self::Insert { .. } => Opcode::Nest(domain_nest::Opcode::Insert),
            Self::Remove { .. } => Opcode::Nest(domain_nest::Opcode::Remove),
            Self::Publish { .. } => Opcode::Nest(domain_nest::Opcode::Publish),
            Self::Retract { .. } => Opcode::Nest(domain_nest::Opcode::Retract),
            Self::Protect { .. } => Opcode::Nest(domain_nest::Opcode::Protect),
        }
    }

    fn valid(self) -> bool {
        match self {
            Self::FaultRoute {
                token, span, route, ..
            } => {
                valid_token(token.get())
                    && valid_token(span.get())
                    && route.is_none_or(FaultRoute::valid)
            }
            Self::CreateFlat {
                license,
                count,
                level,
            } => {
                valid_token(license.get())
                    && crate::model::cell::pages(0, count).is_some()
                    && level
                        .pages()
                        .is_some_and(|grain| count.is_multiple_of(grain))
            }
            Self::CreateNest { license } => valid_token(license.get()),
            Self::DeleteFlat { token, .. } | Self::DeleteNest { token, .. } => {
                valid_token(token.get())
            }
            Self::CreateSpan { token, request, .. } => {
                valid_token(token.get())
                    && request.valid()
                    && match request.placement.at() {
                        crate::model::cell::PlacementAt::Exact(address) => request
                            .count
                            .checked_bytes()
                            .and_then(|bytes| address.get().checked_add(bytes))
                            .is_some(),
                        _ => true,
                    }
            }
            Self::DeleteSpan { token, span, .. } => {
                valid_token(token.get()) && valid_token(span.get())
            }
            Self::ReleaseRange {
                token,
                span,
                offset,
                count,
                ..
            } => {
                valid_token(token.get())
                    && valid_token(span.get())
                    && crate::model::cell::pages(offset, count).is_some()
            }
            Self::Split {
                start,
                parent,
                maintenance,
                ..
            }
            | Self::Merge {
                start,
                parent,
                maintenance,
                ..
            } => {
                Reshape::new(start.page(), parent).is_some()
                    && maintenance.valid()
                    && maintenance.block.flat() == start.flat()
            }
            Self::Inspect(query) => query.valid(),
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
            Self::FaultRoute {
                token,
                nest,
                span,
                expected,
                epoch,
                route,
            } => {
                for word in [
                    token.get() as u64,
                    nest.get() as u64,
                    span.get() as u64,
                    expected.get(),
                    epoch.get(),
                    u64::from(route.is_some()),
                ] {
                    out.word(word)?;
                }
                if let Some(route) = route {
                    out.word(route.sign.get() as u64)?;
                    out.word(route.timeout_ns)?;
                }
                Some(())
            }
            Self::CreateFlat {
                license,
                count,
                level,
            } => {
                out.word(license.get() as u64)?;
                out.word(count as u64)?;
                out.word(level.get() as u64)
            }
            Self::DeleteFlat {
                token,
                flat,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(flat.get() as u64)?;
                out.word(expected.get())
            }
            Self::CreateNest { license } => out.word(license.get() as u64),
            Self::DeleteNest {
                token,
                nest,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(expected.get())
            }
            Self::CreateSpan {
                token,
                nest,
                expected,
                request,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(expected.get())?;
                out.bytes(&request.to_bytes())
            }
            Self::DeleteSpan {
                token,
                nest,
                span,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(span.get() as u64)?;
                out.word(expected.get())
            }
            Self::ReleaseRange {
                token,
                nest,
                span,
                expected,
                offset,
                count,
            } => {
                for word in [
                    token.get() as u64,
                    nest.get() as u64,
                    span.get() as u64,
                    expected.get(),
                    offset as u64,
                    count as u64,
                ] {
                    out.word(word)?;
                }
                Some(())
            }
            Self::Split {
                start,
                expected,
                parent,
                maintenance,
            }
            | Self::Merge {
                start,
                expected,
                parent,
                maintenance,
            } => {
                out.bytes(&start.to_bytes())?;
                out.word(expected.get())?;
                out.word(parent.get() as u64)?;
                maintenance.write(out)
            }
            Self::Inspect(query) => query.write(out),
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
            Opcode::Nest(domain_nest::Opcode::FaultRoute) => Self::FaultRoute {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                expected: SpanVersion::new(input.word()?)?,
                epoch: FaultEpoch::new(input.word()?)?,
                route: match input.word()? {
                    0 => None,
                    1 => Some(FaultRoute {
                        sign: input.token()?,
                        timeout_ns: input.word()?,
                    }),
                    _ => return None,
                },
            },
            Opcode::Flat(domain_flat::Opcode::Create) => Self::CreateFlat {
                license: input.token()?,
                count: input.size()?,
                level: CellLevel::new(input.size()?)?,
            },
            Opcode::Flat(domain_flat::Opcode::Delete) => Self::DeleteFlat {
                token: input.token()?,
                flat: FlatId::new(input.size()?)?,
                expected: FlatVersion::new(input.word()?)?,
            },
            Opcode::Nest(domain_nest::Opcode::Create) => Self::CreateNest {
                license: input.token()?,
            },
            Opcode::Nest(domain_nest::Opcode::Delete) => Self::DeleteNest {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                expected: NestVersion::new(input.word()?)?,
            },
            Opcode::Nest(domain_nest::Opcode::Reserve) => Self::CreateSpan {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                expected: NestVersion::new(input.word()?)?,
                request: CreateSpanRequest::from_bytes(input.bytes::<48>()?)?,
            },
            Opcode::Nest(domain_nest::Opcode::Release) => Self::DeleteSpan {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                expected: SpanVersion::new(input.word()?)?,
            },
            Opcode::Nest(domain_nest::Opcode::ReleaseRange) => Self::ReleaseRange {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                expected: SpanVersion::new(input.word()?)?,
                offset: input.size()?,
                count: input.size()?,
            },
            Opcode::Flat(domain_flat::Opcode::Split) | Opcode::Flat(domain_flat::Opcode::Merge) => {
                let start = SlotPosition::from_bytes(input.bytes::<24>()?)?;
                let expected = FlatVersion::new(input.word()?)?;
                let parent = CellLevel::new(input.size()?)?;
                let maintenance = Maintenance::read(input)?;
                if opcode == Opcode::Flat(domain_flat::Opcode::Split) {
                    Self::Split {
                        start,
                        expected,
                        parent,
                        maintenance,
                    }
                } else {
                    Self::Merge {
                        start,
                        expected,
                        parent,
                        maintenance,
                    }
                }
            }
            Opcode::Flat(domain_flat::Opcode::Inspect)
            | Opcode::Nest(domain_nest::Opcode::Inspect) => {
                let domain = opcode.domain();
                let tag = input.word()?;
                if tag == 3 {
                    Self::InspectResult {
                        domain,
                        request: RequestId::new(input.word()?)?,
                    }
                } else {
                    Self::Inspect(Query::read(domain, tag, input)?)
                }
            }
            Opcode::Flat(domain_flat::Opcode::Occupy) => {
                let (slot, expected) = read_slot(input)?;
                Self::Occupy { slot, expected }
            }
            Opcode::Flat(domain_flat::Opcode::Vacate) => {
                let (slot, expected) = read_slot(input)?;
                Self::Vacate {
                    slot,
                    expected,
                    maintenance: Maintenance::read(input)?,
                }
            }
            Opcode::Flat(domain_flat::Opcode::Block) => {
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
            Opcode::Flat(domain_flat::Opcode::Allow) => Self::Allow {
                maintenance: Maintenance::read(input)?,
            },
            Opcode::Flat(domain_flat::Opcode::Push) | Opcode::Flat(domain_flat::Opcode::Pull) => {
                let (local, local_version) = read_slot(input)?;
                let (remote, remote_version) = read_slot(input)?;
                let maintenance = read_maintenance(input)?;
                if opcode == Opcode::Flat(domain_flat::Opcode::Push) {
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
            Opcode::Nest(domain_nest::Opcode::Insert) => {
                let (target, expected) = read_memo(input)?;
                Self::Insert {
                    target,
                    expected,
                    source: Source::read(input)?,
                }
            }
            Opcode::Nest(domain_nest::Opcode::Remove) => {
                let (memo, expected) = read_memo(input)?;
                Self::Remove { memo, expected }
            }
            Opcode::Nest(domain_nest::Opcode::Publish) => {
                let (memo, expected) = read_memo(input)?;
                Self::Publish {
                    memo,
                    expected,
                    maintenance: read_maintenance(input)?,
                }
            }
            Opcode::Nest(domain_nest::Opcode::Retract) => {
                let (memo, expected) = read_memo(input)?;
                Self::Retract { memo, expected }
            }
            Opcode::Nest(domain_nest::Opcode::Protect) => {
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
        let inspection = matches!(action, Action::InspectResult { .. } | Action::Inspect(_));
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

/// Flat-domain operations. This is the public Flat contract; the shared
/// envelope adds only the RequestId and wire framing.
pub mod flat {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        Create {
            license: PendId,
            count: crate::address::PageCount,
            level: CellLevel,
        },
        Delete {
            token: PendId,
            expected: FlatVersion,
        },
        Inspect(Query),
        InspectResult {
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
        Split {
            start: SlotPosition,
            expected: FlatVersion,
            parent: CellLevel,
            maintenance: Maintenance,
        },
        Merge {
            start: SlotPosition,
            expected: FlatVersion,
            parent: CellLevel,
            maintenance: Maintenance,
        },
    }

    impl TryFrom<super::Action> for Action {
        type Error = ();

        fn try_from(action: super::Action) -> Result<Self, Self::Error> {
            Ok(match action {
                super::Action::CreateFlat {
                    license,
                    count,
                    level,
                } => Self::Create {
                    license,
                    count: crate::address::PageCount::new(count).ok_or(())?,
                    level,
                },
                super::Action::DeleteFlat {
                    token, expected, ..
                } => Self::Delete { token, expected },
                super::Action::Inspect(query) => Self::Inspect(query),
                super::Action::InspectResult {
                    domain: Domain::Flat,
                    request,
                } => Self::InspectResult { request },
                super::Action::Occupy { slot, expected } => Self::Occupy { slot, expected },
                super::Action::Vacate {
                    slot,
                    expected,
                    maintenance,
                } => Self::Vacate {
                    slot,
                    expected,
                    maintenance,
                },
                super::Action::Block {
                    start,
                    count,
                    recover,
                } => Self::Block {
                    start,
                    count,
                    recover,
                },
                super::Action::Allow { maintenance } => Self::Allow { maintenance },
                super::Action::Push {
                    local,
                    local_version,
                    remote,
                    remote_version,
                    maintenance,
                } => Self::Push {
                    local,
                    local_version,
                    remote,
                    remote_version,
                    maintenance,
                },
                super::Action::Pull {
                    local,
                    local_version,
                    remote,
                    remote_version,
                    maintenance,
                } => Self::Pull {
                    local,
                    local_version,
                    remote,
                    remote_version,
                    maintenance,
                },
                super::Action::Split {
                    start,
                    expected,
                    parent,
                    maintenance,
                } => Self::Split {
                    start,
                    expected,
                    parent,
                    maintenance,
                },
                super::Action::Merge {
                    start,
                    expected,
                    parent,
                    maintenance,
                } => Self::Merge {
                    start,
                    expected,
                    parent,
                    maintenance,
                },
                _ => return Err(()),
            })
        }
    }
}

/// Nest-domain operations. Reserve/Release name address occupancy; the
/// internal Span is not part of the public request surface.
pub mod nest {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        FaultRoute {
            token: PendId,
            nest: NestId,
            span: SpanId,
            expected: SpanVersion,
            epoch: FaultEpoch,
            route: Option<FaultRoute>,
        },
        Create {
            license: PendId,
        },
        Delete {
            token: PendId,
            expected: NestVersion,
        },
        Reserve {
            token: PendId,
            nest: NestId,
            expected: NestVersion,
            request: CreateSpanRequest,
        },
        Release {
            token: PendId,
            nest: NestId,
            span: SpanId,
            expected: SpanVersion,
        },
        ReleaseRange {
            token: PendId,
            nest: NestId,
            span: SpanId,
            expected: SpanVersion,
            offset: usize,
            count: usize,
        },
        Inspect(Query),
        InspectResult {
            request: RequestId,
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

    impl TryFrom<super::Action> for Action {
        type Error = ();

        fn try_from(action: super::Action) -> Result<Self, Self::Error> {
            Ok(match action {
                super::Action::FaultRoute {
                    token,
                    nest,
                    span,
                    expected,
                    epoch,
                    route,
                } => Self::FaultRoute {
                    token,
                    nest,
                    span,
                    expected,
                    epoch,
                    route,
                },
                super::Action::CreateNest { license } => Self::Create { license },
                super::Action::DeleteNest {
                    token, expected, ..
                } => Self::Delete { token, expected },
                super::Action::CreateSpan {
                    token,
                    nest,
                    expected,
                    request,
                } => Self::Reserve {
                    token,
                    nest,
                    expected,
                    request,
                },
                super::Action::DeleteSpan {
                    token,
                    nest,
                    span,
                    expected,
                } => Self::Release {
                    token,
                    nest,
                    span,
                    expected,
                },
                super::Action::ReleaseRange {
                    token,
                    nest,
                    span,
                    expected,
                    offset,
                    count,
                } => Self::ReleaseRange {
                    token,
                    nest,
                    span,
                    expected,
                    offset,
                    count,
                },
                super::Action::Inspect(query) => Self::Inspect(query),
                super::Action::InspectResult {
                    domain: Domain::Nest,
                    request,
                } => Self::InspectResult { request },
                super::Action::Insert {
                    target,
                    expected,
                    source,
                } => Self::Insert {
                    target,
                    expected,
                    source,
                },
                super::Action::Remove { memo, expected } => Self::Remove { memo, expected },
                super::Action::Publish {
                    memo,
                    expected,
                    maintenance,
                } => Self::Publish {
                    memo,
                    expected,
                    maintenance,
                },
                super::Action::Retract { memo, expected } => Self::Retract { memo, expected },
                super::Action::Protect {
                    memo,
                    expected,
                    access,
                } => Self::Protect {
                    memo,
                    expected,
                    access,
                },
                _ => return Err(()),
            })
        }
    }
}

/// Shared request envelope for a domain action. The wire Request remains the
/// validation authority; the domain type is preserved for callers and entry
/// dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainRequest<A> {
    wire: Request,
    action: A,
}

impl<A> DomainRequest<A>
where
    A: Copy + Into<Action>,
{
    pub fn new(id: Option<RequestId>, action: A) -> Option<Self> {
        let wire = Request::new(id, action.into())?;
        Some(Self { wire, action })
    }

    pub const fn id(self) -> Option<RequestId> {
        self.wire.id()
    }

    pub const fn action(self) -> A {
        self.action
    }

    pub fn to_bytes(self) -> [u8; Request::WIDTH] {
        self.wire.to_bytes()
    }
}

impl<A> DomainRequest<A>
where
    A: Copy + TryFrom<Action>,
{
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let wire = Request::from_bytes(bytes)?;
        let action = A::try_from(wire.action()).ok()?;
        Some(Self { wire, action })
    }

    pub fn from_call(number: u64, bytes: &[u8]) -> Option<Self> {
        let wire = Request::from_call(number, bytes)?;
        let action = A::try_from(wire.action()).ok()?;
        Some(Self { wire, action })
    }
}

pub type FlatRequest = DomainRequest<flat::Action>;
pub type NestRequest = DomainRequest<nest::Action>;
