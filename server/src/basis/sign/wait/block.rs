use super::*;
use crate::basis::room::Handoff;
use crate::basis::room::conductor::run::run;
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
    life: Weak<Life>,
    dur: Duration,
    wait: Option<WaitCall>,
) -> Result<Blocked, E> {
    if take_beacon(key) {
        return Ok(Blocked::Resume);
    }
    let Some(me) = current().running_task() else {
        return Err(E::busy());
    };
    let _commit = crate::basis::commit();
    if let Some(reason) = crate::basis::unit::lifecycle::take_doomed(me.ident.id) {
        drop(me);
        return Ok(Blocked::Quit(reason));
    }
    {
        let mut sites = sites(key).lock();
        Site::ensure(&mut sites, key, &life).map_err(|()| E::oom())?;
    }
    let ticket = Ticket::next_id();
    let at = wait.as_ref().map_or_else(
        || (dur != Duration::MAX).then(|| clock::now().add(dur).as_ticks()),
        |call| call.deadline(dur),
    );
    if let Some(at) = at {
        hold(ticket, key, &me).map_err(|()| E::oom())?;
        if deadline::tock(ticket.raw(), at).is_err() {
            void(ticket);
            return Err(E::oom());
        }
    }
    drop(me);
    let (task, next_pa) = current().swap();
    task.block(key, ticket, wait);
    let queued = {
        let mut sites = sites(key).lock();
        let queued = match sites.get_mut(&key) {
            None => false,
            Some(site) if Life::dead(&site.life) => false,
            Some(site) if site.pend => {
                site.pend = false;
                false
            }
            Some(site) => {
                site.push_back(task.clone());
                true
            }
        };
        prune(&mut sites, key);
        queued
    };

    if queued {
        drop(task);
    } else {
        void(ticket);
        rise(core::iter::once(task));
    }
    #[cfg(debug_assertions)]
    crate::basis::unit::copies::check_block_heldout();
    Ok(Blocked::Switch(next_pa))
}
