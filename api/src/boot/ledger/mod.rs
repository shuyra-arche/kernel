//! Typed handles handed to the initial Task; domains remain distinct.
mod kind;
pub use kind::ResourceKind;
mod entry;
mod name;

pub use entry::{ENTRY_LEN, Entries, Entry, Header, VERSION, store_entries};
pub use name::{Call, NAME_LEN, Name, Page, Trap};
