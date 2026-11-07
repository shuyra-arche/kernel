mod error;
mod stats;

#[cfg(debug_assertions)]
mod block_kinds;
#[cfg(debug_assertions)]
mod frame_kinds;
#[cfg(debug_assertions)]
mod kind;
#[cfg(debug_assertions)]
mod kinds;
#[cfg(debug_assertions)]
mod mark;
#[cfg(debug_assertions)]
mod mark_cell;

pub use error::Error;
pub(crate) use stats::init;
pub(crate) use stats::record_block_give;
pub(crate) use stats::record_block_take;
pub(crate) use stats::record_frame_give;
pub(crate) use stats::record_frame_take;
pub(crate) use stats::record_pool_give;
pub(crate) use stats::record_pool_take;
pub(crate) use stats::record_spare_give;
pub(crate) use stats::record_spare_take;
pub(crate) use stats::record_spare_total;

#[cfg(debug_assertions)]
pub(crate) use block_kinds::install_block_kinds;
#[cfg(debug_assertions)]
pub(crate) use frame_kinds::install_frame_kinds;
#[cfg(debug_assertions)]
pub(crate) use kind::Kind;
#[cfg(debug_assertions)]
pub(crate) use mark::mark;

#[cfg(not(debug_assertions))]
pub(crate) fn install_frame_kinds(_frames: usize) -> Result<(), super::InitError> {
    Ok(())
}

#[cfg(not(debug_assertions))]
pub(crate) fn install_block_kinds(_base: usize, _len: usize) -> Result<(), super::InitError> {
    Ok(())
}

#[cfg(debug_assertions)]
#[macro_export]
macro_rules! tag {
    ($kind:ident, $v:expr) => {{
        let _m = $crate::machine::allocator::statistics::mark(
            $crate::machine::allocator::statistics::Kind::$kind,
        );
        $v
    }};
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! tag {
    ($kind:ident, $v:expr) => {
        $v
    };
}

pub(crate) use stats::block_occupied;
pub(crate) use stats::frame_occupied;
#[cfg(debug_assertions)]
pub(crate) use stats::kinds;
#[cfg(debug_assertions)]
pub(crate) use stats::spare_available;
#[cfg(debug_assertions)]
pub(crate) use stats::spare_occupied;
