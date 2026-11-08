pub(crate) mod anchor;
pub(crate) mod control;
pub(crate) mod lifecycle;
pub(crate) mod task;
pub(crate) mod team;

use crate::basis::nest::NestError;

pub type NestResult<T> = erra::Result<T, NestError>;
