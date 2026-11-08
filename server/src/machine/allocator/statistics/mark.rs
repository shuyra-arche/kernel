use super::kind::Kind;

#[must_use = "标注靠守卫的生存期起作用：立刻丢掉等于没标"]
pub(crate) fn mark(kind: Kind) -> Mark {
    Mark {
        prev: mark_slot().replace(kind),
    }
}

pub(super) fn current() -> Kind {
    mark_slot().get()
}

pub(crate) struct Mark {
    prev: Kind,
}

impl Drop for Mark {
    fn drop(&mut self) {
        mark_slot().replace(self.prev);
    }
}

struct MarkCell(core::cell::Cell<Kind>);

unsafe impl Sync for MarkCell {}

static MARK: [MarkCell; crate::machine::layout::MAX_HART_SLOTS] =
    [const { MarkCell(core::cell::Cell::new(Kind::Plain)) };
        crate::machine::layout::MAX_HART_SLOTS];

fn mark_slot() -> &'static MarkCell {
    &MARK[crate::machine::hart::hart_id()
        .get()
        .min(crate::machine::layout::MAX_HART_SLOTS - 1)]
}

impl MarkCell {
    fn replace(&self, kind: Kind) -> Kind {
        self.0.replace(kind)
    }

    fn get(&self) -> Kind {
        self.0.get()
    }
}
