use alloc::sync::Arc;
use alloc::sync::Weak;

use super::RegionKind;
use crate::basis::nest::Nest;
use kernel_api::address::VirtAddr;

/// 一笔地址保留：只认住它的那个地址空间（`Weak`，升级失败即失效）。
///
/// 它不参与内存回收的所有权判定；占用表里的 generation 才是回收凭证。
pub struct Reservation {
    nest: Weak<Nest>,
    region: Option<RegionKind>,
    start: VirtAddr,
    size: usize,
    id: usize,
}

impl core::fmt::Debug for Reservation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reservation")
            .field("region", &self.region)
            .field("start", &self.start)
            .field("size", &self.size)
            .finish()
    }
}

impl Reservation {
    pub(crate) fn new(
        nest: Weak<Nest>,
        region: Option<RegionKind>,
        start: VirtAddr,
        size: usize,
        id: usize,
    ) -> Self {
        Self {
            nest,
            region,
            start,
            size,
            id,
        }
    }

    pub fn start(&self) -> VirtAddr {
        self.start
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn region(&self) -> Option<RegionKind> {
        self.region
    }

    /// 占用的 generation。
    pub(crate) fn id(&self) -> usize {
        self.id
    }

    /// 含尾最后一个字节。
    pub(crate) fn last(&self) -> usize {
        self.start.as_usize() + (self.size - 1)
    }

    pub(crate) fn nest(&self) -> Option<Arc<Nest>> {
        self.nest.upgrade()
    }
}

unsafe impl Send for Reservation {}
unsafe impl Sync for Reservation {}

/// A private address reservation with every namespace allocation prepared.
/// No Page or hardware publication can exist before this value is consumed.
pub(crate) struct PreparedReservation {
    nest: Arc<Nest>,
    span: Arc<super::span::Span>,
    node: Option<alloc::boxed::Box<super::super::IndexNode>>,
    binding: Option<Arc<super::super::authority::RangeBinding>>,
    creator: kernel_api::model::identity::TaskId,
    prepared: bool,
    published: bool,
}

impl PreparedReservation {
    pub(in crate::basis::nest) fn new(
        nest: &Arc<Nest>,
        reservation: Reservation,
        level: kernel_api::model::cell::CellLevel,
        access: super::span::Access,
        creator: kernel_api::model::identity::TaskId,
    ) -> Result<Self, super::super::NestError> {
        use super::super::{ActiveRange, IndexNode, NestError, Span};
        let attrs = nest.attrs(reservation.region());
        let mut active = alloc::vec::Vec::new();
        if active.try_reserve(1).is_err() {
            return Err(Nest::abandon(reservation, NestError::OutOfMemory));
        }
        let range = match ActiveRange::new(reservation.start(), reservation.size(), access, access)
        {
            Ok(range) => range,
            Err(error) => return Err(Nest::abandon(reservation, error)),
        };
        active.push(range);
        let span = Span::new(reservation, attrs, active, level, None)?;
        let mut staged = Self {
            nest: nest.clone(),
            span,
            node: None,
            binding: None,
            creator,
            prepared: false,
            published: false,
        };
        staged.node = Some(
            alloc::boxed::Box::try_new(IndexNode::new(staged.span.clone())?)
                .map_err(|_| NestError::OutOfMemory)?,
        );
        staged.binding = Some(
            super::super::authority::RangeBinding::new(staged.span.identity(), nest)
                .map_err(|_| NestError::OutOfMemory)?,
        );
        staged.span.prepare_cleanup()?;
        nest.reservations
            .prepare()
            .map_err(|_| NestError::OutOfMemory)?;
        staged.prepared = true;
        Ok(staged)
    }

    pub(crate) fn span(&self) -> &Arc<super::span::Span> {
        &self.span
    }

    /// Caller holds the actor's Pend serial and COMMIT through grant publication.
    pub(crate) fn publish(
        mut self,
        root: &Arc<crate::basis::pend::Pend>,
    ) -> Result<Arc<super::span::Span>, super::super::NestError> {
        use kernel_api::model::pend::{NestScope, PendScope};
        let mut state = self.nest.state.lock();
        if !state.is_open()
            || !root.alive()
            || root.window()
                != Some((
                    kernel_api::address::PageOffset::ZERO,
                    kernel_api::address::PageCount::new(
                        self.span.initial_size() / kernel_api::address::PAGE_SIZE,
                    )
                    .expect("admitted mapping extent"),
                ))
            || !matches!(root.scope(), PendScope::Nest(NestScope::Reservation { id, .. })
                if id.get() == self.span.identity())
        {
            return Err(super::super::NestError::NoRegion);
        }
        if self.nest.is_external() && self.nest.reservations.competing_reservation(self.creator) {
            return Err(super::super::NestError::Busy);
        }
        if state
            .index
            .first_overlap(
                self.span.initial_start().get(),
                self.span.reservation.last(),
            )
            .is_some()
        {
            return Err(super::super::NestError::AlreadyMapped);
        }
        self.nest.reservations.publish(
            self.binding.take().expect("prepared range identity"),
            self.span.clone(),
            self.creator,
            root,
        );
        state
            .index
            .insert(self.node.take().expect("prepared address index"));
        self.prepared = false;
        self.published = true;
        Ok(self.span.clone())
    }
}

impl Drop for PreparedReservation {
    fn drop(&mut self) {
        if !self.published {
            if self.prepared {
                self.nest.reservations.cancel_prepared();
            }
            // This staged value has never admitted an association or a PTE.
            assert!(Nest::give_back_unpublished(&self.span.reservation));
        }
    }
}
