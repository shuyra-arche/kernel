#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Owner(usize);

impl Owner {
    pub(super) const FREE: Owner = Owner(0);

    pub(super) fn of(hart: usize) -> Owner {
        Owner(hart.checked_add(1).expect("execution owner overflow"))
    }

    pub(super) fn word(self) -> usize {
        self.0
    }

    pub(super) fn from_word(w: usize) -> Owner {
        Owner(w)
    }
}
