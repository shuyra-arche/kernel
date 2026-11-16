use alloc::sync::Arc;

use super::span::Span;
use crate::basis::nest::Nest;

/// A private address span with every namespace allocation prepared.
/// No Page or hardware publication can exist before this value is consumed.
pub(crate) struct PreparedSpan {
    nest: Arc<Nest>,
    span: Arc<Span>,
    node: Option<alloc::boxed::Box<super::super::IndexNode>>,
    binding: Option<Arc<super::super::authority::RangeBinding>>,
    creator: kernel_api::model::identity::TaskId,
    prepared: bool,
    published: bool,
}

impl PreparedSpan {
    pub(in crate::basis::nest) fn new(
        nest: &Arc<Nest>,
        span: Arc<Span>,
        creator: kernel_api::model::identity::TaskId,
    ) -> Result<Self, super::super::NestError> {
        use super::super::{IndexNode, NestError};
        let mut staged = Self {
            nest: nest.clone(),
            span,
            node: None,
            binding: None,
            creator,
            prepared: false,
            published: false,
        };
        staged.node = match alloc::boxed::Box::try_new(IndexNode::new(staged.span.clone())?) {
            Ok(node) => Some(node),
            Err(_) => return Err(NestError::OutOfMemory),
        };
        staged.binding =
            match super::super::authority::RangeBinding::new(staged.span.identity(), nest) {
                Ok(binding) => Some(binding),
                Err(_) => return Err(NestError::OutOfMemory),
            };
        staged.span.prepare_cleanup()?;
        nest.spans.prepare().map_err(|_| NestError::OutOfMemory)?;
        staged.prepared = true;
        Ok(staged)
    }

    pub(crate) fn span(&self) -> &Arc<Span> {
        &self.span
    }

    /// Caller holds the actor's Pend serial and COMMIT through grant publication.
    pub(crate) fn publish(
        mut self,
        root: &Arc<crate::basis::pend::Pend>,
    ) -> Result<Arc<Span>, super::super::NestError> {
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
            || !matches!(root.scope(), PendScope::Nest(NestScope::Span { id, .. })
                if id.get() == self.span.identity())
        {
            return Err(super::super::NestError::NoRegion);
        }
        if self.nest.is_external() && self.nest.spans.competing_span(self.creator) {
            return Err(super::super::NestError::Busy);
        }
        if state
            .index
            .first_overlap(self.span.initial_start().get(), self.span.range.last())
            .is_some()
        {
            return Err(super::super::NestError::AlreadyMapped);
        }
        self.nest
            .version()
            .next()
            .ok_or(super::super::NestError::OutOfMemory)?;
        self.nest.spans.publish(
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
        self.nest.changed();
        Ok(self.span.clone())
    }
}

impl Drop for PreparedSpan {
    fn drop(&mut self) {
        if !self.published {
            if self.prepared {
                self.nest.spans.cancel_prepared();
            }
            // This staged value has never admitted an association or a PTE.
            assert!(self.nest.give_back(&self.span));
        }
    }
}
