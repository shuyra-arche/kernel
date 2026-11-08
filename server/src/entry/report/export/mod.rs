use ::core::sync::atomic::{AtomicUsize, Ordering};

use semihosting::io::Write as _;

use crate::entry::report::record::Report;
use crate::metal::sync::SpinLock;

mod failure;

use failure::Failure;

const EXPORT_NAME: &::core::ffi::CStr = c"kernel-crash.json";

static FAILURE: AtomicUsize = AtomicUsize::new(0);
static HOST: SpinLock<Option<semihosting::fs::File>> = SpinLock::new(None);

fn fail(failure: Failure) {
    let _ = FAILURE.compare_exchange(0, failure as usize, Ordering::AcqRel, Ordering::Acquire);
}

const HOST_WAIT_TICKS: u64 = 10_000;

fn with_host(wait_ticks: Option<u64>, f: impl FnOnce(&mut semihosting::fs::File) -> bool) {
    let budget = wait_ticks.map(|ticks| (crate::metal::clock::now().as_ticks(), ticks));
    let mut g = loop {
        if FAILURE.load(Ordering::Acquire) != 0 {
            return;
        }
        if let Some(g) = HOST.try_lock() {
            break g;
        }
        match budget {
            // Panic may have stopped the hart holding HOST; ordinary tracing must not discard contention.
            Some((start, ticks)) => {
                if crate::metal::clock::now().as_ticks().wrapping_sub(start) >= ticks {
                    fail(Failure::LockTimeout);
                    return;
                }
            }
            None => crate::metal::stop::hush(),
        }
        ::core::hint::spin_loop();
    };
    if FAILURE.load(Ordering::Acquire) != 0 {
        return;
    }
    if g.is_none() {
        match semihosting::fs::File::create(EXPORT_NAME) {
            Ok(f) => *g = Some(f),
            Err(_) => {
                fail(Failure::Create);
                drop(g);
                crate::putln!(
                    "semihosting: cannot create {:?}; host export disabled",
                    EXPORT_NAME
                );
                return;
            }
        }
    }
    let file = g.as_mut().expect("just stored above");
    if !f(file) {
        fail(Failure::Write);
    }
}

fn push(json: &[u8], wait_ticks: Option<u64>) {
    with_host(wait_ticks, |f| {
        f.write_all(json).and_then(|()| f.write_all(b"\n")).is_ok()
    });
}

pub fn export(r: &Report) {
    let Ok(json) = serde_json::to_vec(r) else {
        fail(Failure::Encode);
        return;
    };
    push(&json, Some(HOST_WAIT_TICKS));
}

/// 仅在停机屏障后读取；失败标记不会在本轮清除。
pub(crate) fn complete() -> bool {
    let failure = FAILURE.load(Ordering::Acquire);
    if let Some(failure) = Failure::decode(failure) {
        crate::putln!("semihosting: structured export failed: {failure:?}");
    }
    failure == 0
}
