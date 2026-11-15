#![allow(dead_code)]
#![feature(allocator_ext, reentrant_lock, std_internals)]
#![allow(internal_features)]
extern crate alloc;

#[path = "../../name/allocation.rs"]
mod allocation;
#[path = "../../../support/lock.rs"]
mod host_lock;
mod source;

use kernel_api::model::{flat::FlatId, nest::NestId};
use std::sync::Arc;

// These fixtures supply only discovery keys. They establish no live permission,
// Page retirement, Hart execution or hardware completion.
pub(crate) struct Span(usize);
impl Span {
    fn wake_key(&self) -> Option<(NestId, usize)> {
        Some((NestId::new(1).unwrap(), self.0))
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        source::mechanism::forget();
    }
}
pub(crate) struct RangeBinding;
#[derive(Debug)]
pub(crate) enum NestError {
    OutOfMemory,
}
pub(crate) struct FlatRange(FlatId);
impl FlatRange {
    fn source_id(&self) -> FlatId {
        self.0
    }
}
mod metal {
    pub(crate) mod sync {
        pub(crate) use std::sync::OnceLock;
    }
}
mod basis {
    pub(crate) use crate::host_lock as sync;
    pub(crate) mod flat {
        pub(crate) use crate::FlatRange;
    }
    pub(crate) mod nest {
        pub(crate) use crate::NestError;
        pub(crate) mod authority {
            pub(crate) use crate::RangeBinding;
        }
    }
    pub(crate) mod sign {
        use super::super::{host_lock, source};
        pub(crate) enum WakeKey {
            Reservation {
                space: kernel_api::model::nest::NestId,
                reservation: usize,
            },
        }
        static DELIVERED: host_lock::TestLock<Vec<usize>> = host_lock::TestLock::new(Vec::new());
        pub(crate) fn signal(WakeKey::Reservation { reservation, .. }: WakeKey) {
            DELIVERED.lock().push(reservation);
            if source::AGAIN.swap(false, std::sync::atomic::Ordering::Relaxed) {
                source::mechanism::request();
            }
        }
        pub(crate) fn prepare() {
            let mut delivered = DELIVERED.lock();
            delivered.clear();
            delivered.reserve(4096);
        }
        pub(crate) fn take() -> Vec<usize> {
            std::mem::take(&mut *DELIVERED.lock())
        }
    }
}
