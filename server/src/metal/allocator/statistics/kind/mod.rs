pub(super) mod block;
pub(super) mod frame;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Plain = 0,
    Trap,
    Lazy,
    Heap,
    Stack,
    Image,
    Share,
    Table,
    TrapStack,
    HartFrame,
    Spare,
    Prime,
    Probe,
    Task,
    Team,
    Space,
}

impl Kind {
    pub(crate) const COUNT: usize = Self::ALL.len();

    pub(crate) const ALL: [Kind; 16] = [
        Kind::Plain,
        Kind::Trap,
        Kind::Lazy,
        Kind::Heap,
        Kind::Stack,
        Kind::Image,
        Kind::Share,
        Kind::Table,
        Kind::TrapStack,
        Kind::HartFrame,
        Kind::Spare,
        Kind::Prime,
        Kind::Probe,
        Kind::Task,
        Kind::Team,
        Kind::Space,
    ];

    pub(crate) const fn ix(self) -> usize {
        self as usize
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Kind::Plain => "plain",
            Kind::Trap => "trap",
            Kind::Lazy => "lazy",
            Kind::Heap => "heap",
            Kind::Stack => "stack",
            Kind::Image => "image",
            Kind::Share => "share_view",
            Kind::Table => "table",
            Kind::TrapStack => "trap-stack",
            Kind::HartFrame => "hart-frame",
            Kind::Spare => "spare",
            Kind::Prime => "prime",
            Kind::Probe => "probe",
            Kind::Task => "task",
            Kind::Team => "team",
            Kind::Space => "space",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Kinds([usize; Kind::COUNT]);

impl Kinds {
    pub(crate) const fn new(values: [usize; Kind::COUNT]) -> Self {
        Self(values)
    }

    pub(crate) fn nonzero(&self) -> impl Iterator<Item = (Kind, usize)> + '_ {
        Kind::ALL
            .into_iter()
            .enumerate()
            .filter_map(move |(i, k)| (self.0[i] != 0).then_some((k, self.0[i])))
    }
}

impl core::fmt::Display for Kinds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut any = false;
        for (k, n) in self.nonzero() {
            if any {
                f.write_str(" ")?;
            }
            any = true;
            write!(f, "{}={n}", k.name())?;
        }
        if !any {
            f.write_str("(空)")?;
        }
        Ok(())
    }
}
