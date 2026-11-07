pub(crate) mod capsule;
pub(crate) mod control;
pub(crate) mod copies;
pub(crate) mod join;
pub(crate) mod life;
pub(crate) mod lifecycle;
pub mod space;
pub(crate) mod tally;
pub(crate) mod task;
pub(crate) mod team;

use crate::basis::nest::NestError;

pub type NestResult<T> = erra::Result<T, NestError>;
