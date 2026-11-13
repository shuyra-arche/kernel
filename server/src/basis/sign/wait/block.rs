use super::*;
use crate::basis::room::Handoff;
use crate::basis::room::scheduler::run;
use crate::basis::unit::lifecycle::quit;
use crate::basis::unit::lifecycle::set_exit_reason;

pub(crate) enum Blocked {
    Resume,
    Switch(Option<usize>),
    Quit(usize),
}

pub(crate) fn finish_block(blocked: Blocked) -> Handoff<()> {
    match blocked {
        Blocked::Resume => Handoff::Resume(()),
        Blocked::Switch(next) => Handoff::Switch(next.unwrap_or_else(run)),
        Blocked::Quit(reason) => {
            set_exit_reason(reason);
            Handoff::Switch(quit())
        }
    }
}

pub(crate) fn block_with_call<E: WaitFail>(
    key: WakeKey,
    life: Weak<Anchor>,
    dur: Duration,
    wait: Option<WaitCall>,
) -> Result<Blocked, E> {
    let Some(me) = current().running_task() else {
        return Err(E::busy());
    };
    let _commit = crate::basis::commit();
    if let Some(reason) = crate::basis::unit::lifecycle::take_doomed(me.ident.id) {
        drop(me);
        return Ok(Blocked::Quit(reason));
    }
    if me.realtime() {
        let wait = wait.ok_or_else(E::busy)?;
        drop(me);
        return Ok(Blocked::Switch(current().await_local(key, wait, &_commit)));
    }
    if take_beacon(key) {
        return Ok(Blocked::Resume);
    }
    let registration =
        prepare_wait(&_commit, &me, key, &life, dur, wait.as_ref()).map_err(|()| E::oom())?;
    let ticket = registration.ticket();
    drop(me);
    let (task, next_pa) = current().handoff(
        crate::basis::room::scheduler::slot::Departure::Wait {
            key,
            ticket,
            call: wait,
        },
        &_commit,
    );
    let queued = registration.enroll(&task);

    if queued {
        drop(task);
    } else {
        rise(core::iter::once(task), insert);
    }
    #[cfg(debug_assertions)]
    crate::basis::unit::task::check_block_heldout();
    Ok(Blocked::Switch(next_pa))
}

pub(crate) trait WaitFail: kernel_api::abi::error::FailCode {
    fn busy() -> Self;
    fn oom() -> Self;
}

impl WaitFail for kernel_api::abi::call::room::RoomFail {
    fn busy() -> Self {
        kernel_api::abi::call::room::RoomFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::room::RoomFail::OutOfMemory
    }
}

impl WaitFail for kernel_api::abi::call::unit::UnitFail {
    fn busy() -> Self {
        kernel_api::abi::call::unit::UnitFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::unit::UnitFail::OutOfMemory
    }
}

impl WaitFail for kernel_api::abi::call::clone_view::CloneFail {
    fn busy() -> Self {
        kernel_api::abi::call::clone_view::CloneFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::clone_view::CloneFail::OutOfMemory
    }
}

impl WaitFail for kernel_api::abi::call::sign::SignFail {
    fn busy() -> Self {
        kernel_api::abi::call::sign::SignFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::sign::SignFail::OutOfMemory
    }
}

impl WaitFail for kernel_api::abi::call::share_view::ShareFail {
    fn busy() -> Self {
        kernel_api::abi::call::share_view::ShareFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::share_view::ShareFail::OutOfMemory
    }
}
