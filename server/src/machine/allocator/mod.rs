pub mod bitmap;
pub mod block;
pub mod bump;
pub mod frame;
mod guard;
pub mod hybrid;
pub mod portal;
pub mod spare;
pub mod statistics;

mod error;
mod link;
mod region;
pub(crate) use error::InitError;
pub(crate) use error::InitResult;
#[cfg(debug_assertions)]
pub(crate) use guard::NoAllocation;
pub(crate) use guard::assert_allocation_allowed;
pub(crate) use link::Link;
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
