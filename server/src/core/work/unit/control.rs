//! Administrative execution requests, independent of data-access authority.

#[derive(Clone, Copy, Debug)]
pub(crate) enum Request {
    Pause,
    Resume,
}
