use crate::authority::Pend;
pub(crate) use crate::ledger::PendEntry;
use kernel_api::{
    abi::call::{cell::CellFail, pend::PendFail},
    address::{PageCount, PageOffset},
    model::{
        cell::{CellLevel, MappingId},
        flat::FlatId,
        identity::{PendId, TaskId, TeamId},
        nest::NestId,
        pend::{PendScope, PendStatus},
        view::ViewKind,
    },
};
use std::sync::{
    Arc, Mutex, MutexGuard, Weak,
    atomic::{AtomicBool, Ordering},
};

pub(crate) struct SpinLock<T>(Mutex<T>);
impl<T> SpinLock<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
    pub(crate) fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.0.try_lock().ok()
    }
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
pub(crate) mod spin {
    pub(crate) type SpinLockGuard<'a, T> = std::sync::MutexGuard<'a, T>;
}
pub(crate) struct HoldTable;
impl HoldTable {
    pub(crate) fn new() -> Self {
        Self
    }
    pub(crate) fn retire(&self, _: &Arc<Task>) -> Result<bool, PendFail> {
        Ok(true)
    }
}

pub(crate) struct Team {
    pub(crate) id: TeamId,
    pub(crate) creator: TaskId,
}
pub(crate) struct Ident {
    pub(crate) id: TaskId,
    pub(crate) controller: TaskId,
    pub(crate) team: Team,
}
pub(crate) struct Task {
    pub(crate) ident: Ident,
    pub(crate) administration: super::control::Table,
    pub(crate) pend: super::ledger::Table,
    pub(crate) dead: AtomicBool,
}
impl Task {
    pub(crate) fn new(id: usize) -> Arc<Self> {
        Arc::new(Self {
            ident: Ident {
                id: TaskId::new(id),
                controller: TaskId::new(0),
                team: Team {
                    id: TeamId::new(1),
                    creator: TaskId::new(0),
                },
            },
            administration: super::control::Table::new(),
            pend: super::ledger::Table::new().unwrap(),
            dead: AtomicBool::new(false),
        })
    }
    pub(crate) fn clan(&self, _: TeamId) -> Option<()> {
        None
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        Weak::new()
    }
}

// Namespace metadata and scripted completion only; all Pend authorization is production code.
pub(crate) struct Nest {
    id: NestId,
    pub(crate) open: AtomicBool,
    pub(crate) closed: AtomicBool,
    pub(crate) adopted: AtomicBool,
    pub(crate) external: AtomicBool,
    pub(crate) busy: AtomicBool,
    pub(crate) mappings: Mutex<Vec<(MappingId, PendStatus)>>,
    spans: Mutex<Vec<(MappingId, Arc<Span>, Weak<Pend>)>>,
    publications: Mutex<Vec<(MappingId, Arc<Pend>)>>,
    pub(crate) withdrawn: Mutex<Vec<(MappingId, (PageOffset, PageCount), Weak<Pend>)>>,
    pub(crate) released: Mutex<Vec<(MappingId, PageCount)>>,
}
pub(crate) struct Span {
    count: PageCount,
    level: CellLevel,
}
impl Span {
    pub(crate) fn range(&self, offset: PageOffset, count: PageCount) -> Result<(), ()> {
        let grain = self.level.pages().ok_or(())?;
        if !offset.get().is_multiple_of(grain)
            || !count.get().is_multiple_of(grain)
            || offset
                .get()
                .checked_add(count.get())
                .is_none_or(|end| end > self.count.get())
        {
            return Err(());
        }
        Ok(())
    }
}
impl Nest {
    pub(crate) fn new(id: usize) -> Arc<Self> {
        Arc::new(Self {
            id: NestId::new(id).unwrap(),
            open: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            adopted: AtomicBool::new(false),
            external: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            mappings: Mutex::new(Vec::new()),
            spans: Mutex::new(Vec::new()),
            publications: Mutex::new(Vec::new()),
            withdrawn: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
        })
    }
    pub(crate) fn install(
        &self,
        id: MappingId,
        count: PageCount,
        level: CellLevel,
        root: &Arc<Pend>,
    ) {
        self.mappings.lock().unwrap().push((id, PendStatus::Active));
        self.spans.lock().unwrap().push((
            id,
            Arc::new(Span { count, level }),
            Arc::downgrade(root),
        ));
    }
    pub(crate) fn publish(&self, id: MappingId, publisher: &Arc<Pend>) {
        publisher.retain_route().unwrap();
        self.publications
            .lock()
            .unwrap()
            .push((id, publisher.clone()));
    }
    fn complete(&self, id: MappingId, publisher: Option<&Arc<Pend>>) {
        self.publications.lock().unwrap().retain(|(known, route)| {
            if *known != id || publisher.is_some_and(|publisher| !Arc::ptr_eq(route, publisher)) {
                return true;
            }
            // The fixture supplies completion only for the selected publication.
            unsafe { route.release_route() };
            false
        });
    }
    pub(crate) fn id(&self) -> NestId {
        self.id
    }
    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(crate) fn is_adopted(&self) -> bool {
        self.adopted.load(Ordering::Acquire)
    }
    pub(crate) fn is_external(&self) -> bool {
        self.external.load(Ordering::Acquire)
    }
    pub(crate) fn mapping(&self, id: MappingId) -> Option<Arc<Span>> {
        self.spans
            .lock()
            .unwrap()
            .iter()
            .find(|(known, _, _)| *known == id)
            .map(|(_, span, _)| span.clone())
    }
    pub(crate) fn mapping_count(&self, id: MappingId) -> Option<PageCount> {
        Some(self.mapping(id)?.count)
    }
    pub(crate) fn mapping_root(&self, id: MappingId, root: &Arc<Pend>) -> bool {
        self.spans
            .lock()
            .unwrap()
            .iter()
            .any(|(known, _, original)| *known == id && original.ptr_eq(&Arc::downgrade(root)))
    }
    pub(crate) fn mapping_open(&self, id: MappingId) -> bool {
        self.is_open() && self.mapping_status(id) == PendStatus::Active
    }
    pub(crate) fn mapping_status(&self, id: MappingId) -> PendStatus {
        self.mappings
            .lock()
            .unwrap()
            .iter()
            .find(|(known, _)| *known == id)
            .map_or(PendStatus::Dead, |(_, status)| *status)
    }
    pub(crate) fn withdraw_mapping(
        &self,
        id: MappingId,
        window: (PageOffset, PageCount),
        publisher: &Arc<Pend>,
    ) -> Result<(), PendFail> {
        if self.busy.load(Ordering::Acquire) {
            return Err(PendFail::Busy);
        }
        self.withdrawn
            .lock()
            .unwrap()
            .push((id, window, Arc::downgrade(publisher)));
        self.complete(id, Some(publisher));
        Ok(())
    }
    pub(crate) fn release_mapping(&self, id: MappingId) -> Result<(), CellFail> {
        let count = self.mapping_count(id).ok_or(CellFail::Denied)?;
        if self.busy.load(Ordering::Acquire) {
            return Err(CellFail::Busy);
        }
        self.complete(id, None);
        self.released.lock().unwrap().push((id, count));
        self.mappings
            .lock()
            .unwrap()
            .iter_mut()
            .find(|(known, _)| *known == id)
            .unwrap()
            .1 = PendStatus::Dead;
        Ok(())
    }
    pub(crate) fn retire_mapping(&self, raw: usize) {
        let removable = self.spans.lock().unwrap().iter().any(|(id, _, root)| {
            id.get() == raw
                && self.mapping_status(*id) == PendStatus::Dead
                && root
                    .upgrade()
                    .is_none_or(|root| root.status() == PendStatus::Dead)
        });
        if removable {
            self.spans
                .lock()
                .unwrap()
                .retain(|(id, _, _)| id.get() != raw);
            self.mappings
                .lock()
                .unwrap()
                .retain(|(id, _)| id.get() != raw);
        }
    }
    pub(crate) fn retire(&self) -> Result<(), ()> {
        self.open.store(false, Ordering::Release);
        self.closed.store(true, Ordering::Release);
        Ok(())
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        Weak::new()
    }
}

// Unexercised resource families cannot authorize anything in this fixture.
fn unsupported<T>() -> T {
    panic!("resource family is outside the registration fixture")
}
pub(crate) struct Anchor;
pub(crate) struct Source;
impl Source {
    pub(crate) fn flat(&self) -> &Self {
        unsupported()
    }
    pub(crate) fn is_explicit(&self) -> bool {
        unsupported()
    }
    pub(crate) fn source_id(&self) -> FlatId {
        unsupported()
    }
    pub(crate) fn page_count(&self) -> PageCount {
        unsupported()
    }
    pub(crate) fn is_borrowed(&self) -> bool {
        unsupported()
    }
    pub(crate) fn range(&self, _: PageOffset, _: PageCount) -> Result<(), ()> {
        unsupported()
    }
}
pub(crate) struct SourceAuthorization;
impl SourceAuthorization {
    pub(crate) fn check_publish(&self, _: PageOffset, _: PageCount) -> Result<(), PendFail> {
        unsupported()
    }
    pub(crate) fn blocked_by(&self, _: &Arc<Pend>) -> bool {
        unsupported()
    }
    pub(crate) fn begin_withdraw(&self) -> Result<(), PendFail> {
        unsupported()
    }
}
pub(crate) struct Sharing {
    pub(crate) id: usize,
}
impl Sharing {
    pub(crate) fn owner(&self) -> TaskId {
        unsupported()
    }
    pub(crate) fn alive(&self) -> bool {
        unsupported()
    }
    pub(crate) fn source(&self) -> Source {
        unsupported()
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        unsupported()
    }
    pub(crate) fn authorization(&self) -> Option<Arc<SourceAuthorization>> {
        unsupported()
    }
    pub(crate) fn close_maps(&self, _: Option<PendId>) -> Result<(), PendFail> {
        unsupported()
    }
    pub(crate) fn register_pend(&self, _: PendId, _: Arc<Pend>) -> Result<(), PendFail> {
        unsupported()
    }
    pub(crate) fn release_pend(&self, _: PendId) {
        unsupported()
    }
}
pub(crate) struct SharingOperation;
impl SharingOperation {
    pub(crate) fn acquire(_: &Arc<Sharing>) -> Option<Self> {
        unsupported()
    }
}
pub(crate) struct Endpoint {
    pub(crate) id: usize,
}
impl Endpoint {
    pub(crate) fn owner(&self) -> TaskId {
        unsupported()
    }
    pub(crate) fn alive(&self) -> bool {
        unsupported()
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        unsupported()
    }
}
pub(crate) struct CloneView;
#[derive(Clone)]
pub(crate) enum View {
    Clone(Arc<CloneView>),
    Share(Arc<Sharing>),
}
impl View {
    pub(crate) fn kind(&self) -> ViewKind {
        unsupported()
    }
    pub(crate) fn owner(&self) -> TaskId {
        unsupported()
    }
    pub(crate) fn id(&self) -> usize {
        unsupported()
    }
    pub(crate) fn life(&self) -> Weak<Anchor> {
        unsupported()
    }
    pub(crate) fn alive(&self) -> bool {
        unsupported()
    }
    pub(crate) fn seal(&self) {
        unsupported()
    }
    pub(crate) fn clone_view(&self) -> Option<Arc<CloneView>> {
        unsupported()
    }
    pub(crate) fn share_view(&self) -> Option<Arc<Sharing>> {
        unsupported()
    }
    pub(crate) fn same(&self, _: &Self) -> bool {
        unsupported()
    }
    pub(crate) fn new_pend(
        &self,
        _: kernel_api::model::modifiable::Modifiable,
        _: PendId,
        _: Option<PendScope>,
    ) -> Result<Arc<Pend>, PendFail> {
        unsupported()
    }
    pub(crate) fn release_pend(&self, _: PendId) {
        unsupported()
    }
}
pub(crate) enum WakeKey {
    ExternalNest { id: NestId },
    Pends { task: TaskId },
    Inspect { task: TaskId, token: usize },
}
impl WakeKey {
    pub(crate) fn seal(_: ViewKind, _: usize) -> Self {
        unsupported()
    }
}
