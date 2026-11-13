//! Holding provenance follows the exact Page through Flat commits. Weak source
//! and Memo discovery edges avoid ownership cycles; only live Pends authorize.
use super::*;
use crate::basis::{
    pend::{Target, authority::carried::Carried},
    unit::task::Task,
};
use kernel_api::model::pend::FlatRights;

pub(crate) struct Provenance {
    grant: SpinLock<Carried>,
    source: Weak<SourceAuthorization>,
    geometry: Arc<Geometry>,
    ceiling: Access,
    previous: Option<Arc<Self>>,
    depth: usize,
    uses: SpinLock<Vec<Weak<RangeBinding>>>,
    introduced: Option<Weak<RangeBlock>>,
}
impl Provenance {
    /// Returning through the exact original position and its live branch (or
    /// real ancestor) restores the pre-transfer chain. An unrelated slot or
    /// sibling grant cannot erase restrictions by imitating its coordinates.
    pub(crate) fn prepare(
        actor: &Arc<Task>,
        token: PendId,
        range: &FlatRange,
        target_token: PendId,
        target: &FlatRange,
        block: Option<&Arc<RangeBlock>>,
        new_import: bool,
    ) -> Result<(Option<Arc<Self>>, bool), PendFail> {
        let entry = crate::basis::pend::locate(actor, target_token).ok_or(PendFail::Denied)?;
        let Target::Flat(meta) = entry.target() else {
            return Err(PendFail::Denied);
        };
        if !crate::basis::pend::live(actor) || meta.source().source_id() != target.source_id() {
            return Err(PendFail::Denied);
        }
        let source = meta.authorization().ok_or(PendFail::Denied)?;
        let offset = PageOffset::new(
            target
                .index()
                .get()
                .checked_sub(meta.source().index().get())
                .ok_or(PendFail::Denied)?,
        );
        let previous = range.provenance();
        let mut next = previous.as_deref();
        while let Some(origin) = next {
            if core::ptr::eq(origin.source.as_ptr(), source.as_ref())
                && origin
                    .grant
                    .lock()
                    .returns_to(entry.authority(), offset, target.page_count())
            {
                let release = block.is_some_and(|block| {
                    origin
                        .introduced
                        .as_ref()
                        .is_some_and(|b| core::ptr::eq(b.as_ptr(), block.as_ref()))
                });
                return Ok((origin.previous.clone(), release));
            }
            next = origin.previous.as_deref();
        }
        Ok((
            Some(Self::capture(
                actor,
                token,
                range,
                block.filter(|_| new_import),
            )?),
            false,
        ))
    }
    /// Only TRANSFER is required. Missing data/release/distribution permission is
    /// retained as a restriction and checked when that separate action is used.
    fn capture(
        actor: &Arc<Task>,
        token: PendId,
        range: &FlatRange,
        introduced: Option<&Arc<RangeBlock>>,
    ) -> Result<Arc<Self>, PendFail> {
        let entry = crate::basis::pend::locate(actor, token).ok_or(PendFail::Denied)?;
        let Target::Flat(meta) = entry.target() else {
            return Err(PendFail::Denied);
        };
        if meta.source().source_id() != range.source_id() {
            return Err(PendFail::Denied);
        }
        let source = meta.authorization().ok_or(PendFail::Denied)?;
        let offset = PageOffset::new(
            range
                .index()
                .get()
                .checked_sub(meta.source().index().get())
                .ok_or(PendFail::Denied)?,
        );
        let count = range.page_count();
        let previous = range.provenance();
        let depth = previous.as_ref().map_or(1, |p| p.depth + 1);
        if depth > 64 || !crate::basis::pend::live(actor) {
            return Err(PendFail::Denied);
        }
        let origin = Arc::try_new(Self {
            grant: SpinLock::new(Carried::new(
                entry.authority(),
                actor.ident.id,
                offset,
                count,
            )?),
            source: Arc::downgrade(&source),
            geometry: Geometry::new(offset, count)?,
            ceiling: source.ceiling(),
            previous,
            depth,
            uses: SpinLock::new(Vec::new()),
            introduced: introduced.map(Arc::downgrade),
        })
        .map_err(|_| PendFail::OutOfMemory)?;
        let mut known = source.origins.lock();
        known.retain(|p| p.strong_count() != 0);
        if known.len() >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
        known.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        known.push(Arc::downgrade(&origin));
        Ok(origin)
    }
    fn each(&self, mut visit: impl FnMut(&Self) -> Result<(), PendFail>) -> Result<(), PendFail> {
        let mut next = Some(self);
        while let Some(origin) = next {
            visit(origin)?;
            next = origin.previous.as_deref();
        }
        Ok(())
    }
    pub(crate) fn contains(&self, expected: &Self) -> bool {
        let mut next = Some(self);
        while let Some(origin) = next {
            if core::ptr::eq(origin, expected) {
                return true;
            }
            next = origin.previous.as_deref();
        }
        false
    }
    pub(crate) fn check(
        &self,
        actor: TaskId,
        access: Option<Access>,
        rights: FlatRights,
        export: bool,
    ) -> Result<(), PendFail> {
        self.each(|p| {
            let source = p.source.upgrade().ok_or(PendFail::Denied)?;
            let mode = access.map(|a| {
                if a.contains(Access::WRITE) {
                    Modifiable::Yes
                } else {
                    Modifiable::No
                }
            });
            if source.revoked()
                || access.is_some_and(|a| !p.ceiling.contains(a))
                || !p.grant.lock().permits(actor, rights, mode, export)
            {
                return Err(PendFail::Denied);
            }
            Ok(())
        })
    }
    pub(crate) fn retiring(&self) -> bool {
        self.each(|p| {
            if !p.grant.lock().live() || p.source.upgrade().is_none_or(|s| s.revoked()) {
                Err(PendFail::Denied)
            } else {
                Ok(())
            }
        })
        .is_err()
    }
    pub(crate) fn admit(&self, owner: Option<&Pend>) -> Result<(), PendFail> {
        self.each(|p| {
            let source = p.source.upgrade().ok_or(PendFail::Denied)?;
            let (offset, count) = p.geometry.window();
            source.check_move(offset, count, owner)
        })
    }
    /// Serialize original-source Narrow/retirement with publication preparation.
    /// These are operation leases, not locks; no source lock crosses a fence.
    pub(crate) fn operations(
        &self,
        direct: &SourceAuthorization,
    ) -> Result<Vec<crate::basis::flat::sharing::SharingOperation>, PendFail> {
        let mut owners = Vec::new();
        let mut operations = Vec::new();
        owners
            .try_reserve(self.depth)
            .map_err(|_| PendFail::OutOfMemory)?;
        operations
            .try_reserve(self.depth)
            .map_err(|_| PendFail::OutOfMemory)?;
        self.each(|p| {
            let source = p.source.upgrade().ok_or(PendFail::Denied)?;
            if core::ptr::eq(source.as_ref(), direct)
                || owners.iter().any(|known| Arc::ptr_eq(known, &source))
            {
                return Ok(());
            }
            let (meta, _, _) = source.root().ok_or(PendFail::Denied)?;
            operations.push(
                crate::basis::flat::sharing::SharingOperation::acquire(&meta)
                    .ok_or(PendFail::Busy)?,
            );
            owners.push(source);
            Ok(())
        })?;
        Ok(operations)
    }
    pub(crate) fn remember(&self, binding: &Arc<RangeBinding>) -> Result<(), PendFail> {
        self.each(|p| {
            let mut uses = p.uses.lock();
            uses.retain(|u| u.strong_count() != 0);
            if uses
                .iter()
                .any(|u| core::ptr::eq(u.as_ptr(), binding.as_ref()))
            {
                return Ok(());
            }
            if uses.len() >= 1024 {
                return Err(PendFail::OutOfMemory);
            }
            uses.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
            uses.push(Arc::downgrade(binding));
            Ok(())
        })
    }
    fn retire(self: &Arc<Self>, block: Option<&RangeBlock>) -> Result<(), PendFail> {
        let bindings = {
            let known = self.uses.lock();
            let mut bindings = Vec::new();
            bindings
                .try_reserve(known.len().min(4))
                .map_err(|_| PendFail::OutOfMemory)?;
            bindings.extend(known.iter().filter_map(Weak::upgrade).take(4));
            bindings
        };
        for binding in bindings {
            let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
            nest.withdraw_provenance(&binding, self, block)?;
            self.uses
                .lock()
                .retain(|u| u.strong_count() != 0 && !core::ptr::eq(u.as_ptr(), binding.as_ref()));
        }
        if self.uses.lock().iter().any(|u| u.strong_count() != 0) {
            Err(PendFail::Busy)
        } else {
            Ok(())
        }
    }
}
impl SourceAuthorization {
    fn origins(&self) -> Result<Vec<Arc<Provenance>>, PendFail> {
        let mut known = self.origins.lock();
        known.retain(|p| p.strong_count() != 0);
        let mut origins = Vec::new();
        origins
            .try_reserve(known.len())
            .map_err(|_| PendFail::OutOfMemory)?;
        origins.extend(known.iter().filter_map(Weak::upgrade));
        Ok(origins)
    }
    pub(super) fn failed_origins(&self, block: &RangeBlock) -> Result<bool, PendFail> {
        let (offset, count) = block.window();
        for origin in self.origins()? {
            if !origin.geometry.overlaps(offset, count) {
                continue;
            }
            let mut index = 0;
            loop {
                let binding = origin.uses.lock().get(index).cloned();
                let Some(binding) = binding else { break };
                index += 1;
                if let Some(binding) = binding.upgrade() {
                    if let Some(nest) = binding.nest().upgrade() {
                        if nest.provenance_failed(&binding, &origin) {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }
    pub(crate) fn retire_origins(
        &self,
        branch: Option<&Pend>,
        block: Option<&RangeBlock>,
    ) -> Result<(), PendFail> {
        for origin in self.origins()? {
            if branch.is_some_and(|branch| !origin.grant.lock().authority().descends_from(branch)) {
                continue;
            }
            if block.is_some_and(|b| {
                let (o, c) = b.window();
                !origin.geometry.overlaps(o, c)
            }) {
                continue;
            }
            origin.retire(block)?;
        }
        Ok(())
    }
    /// Called at the already-settled Block takeover commit. No allocation or
    /// hardware effect; descendants retain their immutable original ceiling.
    pub(crate) fn recover_origins(&self, actor: TaskId, authority: &Arc<Pend>, block: &RangeBlock) {
        let (offset, count) = block.window();
        let known = self.origins.lock();
        for origin in known.iter().filter_map(Weak::upgrade) {
            if origin.geometry.overlaps(offset, count) {
                origin.grant.lock().recover(actor, authority);
            }
        }
    }
}
