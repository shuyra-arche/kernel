//! Named capabilities handed to the initial task.
mod entry;
mod name;

pub use entry::{ENTRY_LEN, Entries, Entry, Header, VERSION, store_entries};
pub use name::{Call, NAME_LEN, Name, Page, Trap};
