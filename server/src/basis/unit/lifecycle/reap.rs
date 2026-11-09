use alloc::sync::Arc;

use kernel_api::abi::call::unit::NOTE_MAX;
use kernel_api::model::identity::TaskId;

use super::diagnostics;
use crate::basis::nest::Nest;
use crate::basis::room::scheduler::slot::current;
use crate::basis::unit::lifecycle::tally;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskExitCause;
use crate::basis::unit::task::TaskState;
use crate::basis::unit::task::chain::Chain;
use crate::metal::sync::Level;
use crate::metal::sync::OnceLock;
use crate::metal::sync::SpinLock;
use kernel_api::address::VirtAddr;

use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::sign::wipe;

pub(super) static HUSKS: SpinLock<Husks> = SpinLock::new_level(Level::L3, Husks::new());

pub(super) struct Husks {
    chain: Chain,
}

impl Husks {
    const fn new() -> Self {
        Self {
            chain: Chain::new(),
        }
    }

    fn enqueue(&mut self, task: Arc<Task>) {
        debug_assert!(
            task.reaped(),
            "躯壳容器只收 Reaped 任务，且入壳前不得挂在链上"
        );
        self.chain.enqueue(task);
    }

    fn dequeue(&mut self) -> Option<Arc<Task>> {
        self.chain.dequeue()
    }

    #[cfg(debug_assertions)]
    pub(super) fn len(&self) -> usize {
        self.chain.len()
    }

    pub(super) fn take(&mut self) -> Option<crate::basis::unit::task::chain::Drain> {
        self.chain.take()
    }
}

pub(super) fn reap(task: Arc<Task>, cause: TaskExitCause, reason: usize) {
    let (cause, reason) = {
        let _commit = crate::basis::commit();
        let mut state = task.state.lock();
        if state.exit().is_some() {
            return;
        }
        let (cause, reason) = state.dying().unwrap_or((cause, reason));
        state.doom(None, cause, reason);
        drop(state);
        // Last member closes external admission before fallible Pend/CPU cleanup.
        // Parent Join/Oust and Driver cooperation are not prerequisites.
        if task
            .ident
            .team
            .node
            .tasks
            .lock()
            .iter()
            .all(|member| member.state.lock().is_dead())
        {
            task.ident.team.external.close();
        }
        (cause, reason)
    };
    crate::basis::unit::team::exit::member_dying(&task, reason);
    task.release_extensions();
    hooked(&task);
    {
        let _commit = crate::basis::commit();
        task.transform_checked(TaskState::reaped(cause, reason));
    }
    sign::signal(WakeKey::Task { id: task.ident.id });
    task.ident.team.node.notify();
    HUSKS.lock().enqueue(task);
}

pub fn quit() -> usize {
    let cond = current();
    let requested = super::take_exit_reason();
    let (exited, _next_pa, reason) = {
        let _commit = crate::basis::commit();
        let (task, next) = cond.handoff(
            crate::basis::room::scheduler::slot::Departure::Exit { reason: requested },
            &_commit,
        );
        let (_, reason) = task.state.lock().dying().expect("committed exit");
        (task, next, reason)
    };
    debug_assert!(
        exited.on_hart().is_some() || exited.doomed(),
        "running 容器里不是在核上的任务"
    );
    let (note_va, note_len) = super::take_exit_note();
    let tid = exited.ident.id;
    let mut buf = [0u8; NOTE_MAX];
    let text = note_out(
        &exited.ident.team.nest,
        tid,
        reason,
        note_va,
        note_len,
        &mut buf,
    );
    let owner = {
        let _commit = crate::basis::commit();
        exited
            .ident
            .team
            .node
            .member(tid)
            .and_then(|m| m.receipts.lock().first().map(|r| r.owner))
    };
    diagnostics::note(tid, reason, text, owner);
    reap(exited, TaskExitCause::Reap, reason);
    crate::basis::room::scheduler::run()
}

fn note_out<'a>(
    space: &Nest,
    tid: TaskId,
    reason: usize,
    va: usize,
    len: usize,
    buf: &'a mut [u8; NOTE_MAX],
) -> &'a str {
    if len == 0 {
        return "";
    }
    let n = len.min(NOTE_MAX);
    if !space.copy_in(&mut buf[..n], VirtAddr::wrap(va)) {
        crate::putln!(
            "exit tid={} reason={reason:#x} note=<unreadable {len} bytes at {va:#x}>",
            tid.get()
        );
        return "";
    }
    match core::str::from_utf8(&buf[..n]) {
        Ok(text) => {
            crate::putln!("exit tid={} reason={reason:#x} note: {text}", tid.get());
            text
        }
        Err(_) => {
            crate::putln!(
                "exit tid={} reason={reason:#x} note: <non-utf8 note>",
                tid.get()
            );
            "<non-utf8 note>"
        }
    }
}

pub(crate) fn bury() {
    // Bound this service turn's outer work. Unsettled Tasks retain their place
    // for a later turn; each Task's recursive retirement is a separate M2 bound.
    for _ in 0..4 {
        let z = {
            let mut husks = HUSKS.lock();
            let Some(z) = husks.dequeue() else {
                break;
            };
            z
        };
        if z.retire().is_err() {
            HUSKS.lock().enqueue(z);
            break;
        }
        if let Some(member) = z.ident.team.node.member(z.ident.id) {
            member
                .reclaimed
                .store(true, core::sync::atomic::Ordering::Release);
        }
        wipe(WakeKey::Task { id: z.ident.id });
        wipe(WakeKey::Pends { task: z.ident.id });
        z.ident.team.prune_tasks();
        let _pruned = crate::basis::unit::task::registry::prune();
        drop(z);
        tally::exit();
    }
    if !HUSKS.lock().chain.is_empty() {
        // Keep the continuation live even with no competing ready Task to
        // shorten the ordinary scheduling ceiling. Busy retirement retains all
        // hardware and Cell ownership until a later attempt actually settles.
        crate::basis::room::deadline::retry();
    }
}

type Hook = fn(&Arc<Task>);

static HOOKS: OnceLock<&'static [Hook]> = OnceLock::new();

pub(crate) fn hook(hooks: &'static [Hook]) {
    let _ = HOOKS.set(hooks);
}

fn hooked(task: &Arc<Task>) {
    {
        let _commit = crate::basis::commit();
        task.administration.clear();
    }
    if let Some(hooks) = HOOKS.get() {
        for h in hooks.iter() {
            h(task);
        }
    }
}
