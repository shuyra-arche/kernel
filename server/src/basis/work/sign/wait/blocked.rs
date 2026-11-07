use crate::basis::work::room::Handoff;
use crate::basis::work::room::conductor::run::run;
use crate::basis::work::unit::lifecycle::{quit, set_exit_reason};

pub(crate) enum Blocked {
    Resume,
    Switch(Option<usize>),
    Quit(usize),
}

pub(crate) fn finish_block(blocked: Blocked) -> Handoff<()> {
    match blocked {
        Blocked::Resume => Handoff::Resume(()),
        Blocked::Switch(next) => Handoff::Switch(next.unwrap_or_else(run)),
        Blocked::Quit(reason) => {
            set_exit_reason(reason);
            Handoff::Switch(quit())
        }
    }
}
