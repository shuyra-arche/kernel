pub mod bitmap;
pub mod block;
pub mod bump;
pub mod frame;
pub mod hybrid;
mod no_allocation;
pub mod portal;
pub mod spare;
pub mod statistics;

mod error;
mod facts;
mod link;
mod region;
pub(crate) use error::InitError;
pub(crate) use error::InitResult;
pub(crate) use facts::MAX_RESERVED;
pub(crate) use facts::MachineFacts;
pub(crate) use link::Link;
#[cfg(debug_assertions)]
pub(crate) use no_allocation::NoAllocation;
pub(crate) use no_allocation::assert_allocation_allowed;
pub(crate) use region::Region;

pub fn init() -> InitResult<()> {
    bump::init()?;
    portal::switch(portal::Backend::Bump);

    statistics::init().map_err(|error| {
        erra::Error::new(
            "initializing allocator statistics",
            match error {
                statistics::Error::AlreadyInitialized => InitError::AlreadyInitialized,
                statistics::Error::OutOfMemory => InitError::OutOfMemory,
            },
        )
    })?;

    hybrid::init()?;
    portal::switch(portal::Backend::Hybrid);
    spare::init()?;

    statistics::record_spare_total(spare::spare().total_bytes());

    Ok(())
}
