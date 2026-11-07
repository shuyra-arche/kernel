//! Administrative execution requests and qualifications, independent of data access.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Request {
    Pause,
    Resume,
}
mod table;
pub(crate) use table::Table;
pub(crate) use table::delegate;
pub(crate) fn adjacent(caller: &super::task::Task, target: &super::task::Task) -> bool {
    caller.ident.team.id == target.ident.team.id
        || target.ident.team.creator == caller.ident.id
        || caller.ident.team.creator == target.ident.id
}

pub(crate) fn manages(caller: &super::task::Task, target: &super::task::Task) -> bool {
    caller.ident.id == target.ident.id
        || target.ident.controller == caller.ident.id
        || caller.clan(target.ident.team.id).is_some()
}
