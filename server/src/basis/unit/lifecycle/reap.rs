use alloc::sync::Arc;

use kernel_api::abi::call::unit::NOTE_MAX;
use kernel_api::model::identity::TaskId;

use super::diagnostics;
use crate::basis::room::chain::Chain;
use crate::basis::room::conductor::current;
use crate::basis::unit::lifecycle::tally;
use crate::basis::unit::space::Space;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskExitCause;
use crate::basis::unit::task::TaskState;
use crate::machine::sync::Level;
use crate::machine::sync::OnceLock;
use crate::machine::sync::SpinLock;
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

    fn push(&mut self, task: Arc<Task>) {
        debug_assert!(
            task.reaped(),
            "躯壳容器只收 Reaped 任务，且入壳前不得挂在链上"
        );
        self.chain.push(task);
    }

    fn pop(&mut self) -> Option<Arc<Task>> {
        self.chain.pop()
    }

    #[cfg(debug_assertions)]
    pub(super) fn len(&self) -> usize {
        self.chain.len()
    }

    pub(super) fn take(&mut self) -> Option<Arc<Task>> {
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
        // Last member stops device admission before fallible Pend/CPU cleanup.
        // Parent Join/Oust and Driver cooperation are not prerequisites.
        if task
            .ident
            .team
            .life
            .tasks
            .lock()
            .iter()
            .all(|member| member.state.lock().is_dead())
        {
            task.ident.team.space.dma.close();
        }
        (cause, reason)
    };
    crate::basis::unit::team::exit::member_dying(&task, reason);
    hooked(&task);
    {
        let _commit = crate::basis::commit();
        task.transform_checked(TaskState::reaped(cause, reason));
    }
    sign::signal(WakeKey::Task { id: task.ident.id });
    task.ident.team.life.notify();
    HUSKS.lock().push(task);
}

pub fn quit() -> usize {
    let cond = current();
    let requested = super::take_exit_reason();
    let (exited, _next_pa, reason) = {
        let _commit = crate::basis::commit();
        let (task, next) = cond.swap();
        let mut state = task.state.lock();
        let (cause, reason) = state.dying().unwrap_or((TaskExitCause::Reap, requested));
        state.doom(None, cause, reason);
        drop(state);
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
        &exited.ident.team.space,
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
            .life
            .member(tid)
            .and_then(|m| m.receipts.lock().first().map(|r| r.owner))
    };
    diagnostics::note(tid, reason, text, owner);
    reap(exited, TaskExitCause::Reap, reason);
    crate::basis::room::conductor::run::run()
}

fn note_out<'a>(
    space: &Space,
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
    loop {
        let z = {
            let mut husks = HUSKS.lock();
            let Some(z) = husks.pop() else {
                break;
            };
            z
        };
        if z.retire().is_err() {
            HUSKS.lock().push(z);
            break;
        }
        if let Some(member) = z.ident.team.life.member(z.ident.id) {
            member
                .reclaimed
                .store(true, core::sync::atomic::Ordering::Release);
        }
        wipe(WakeKey::Task { id: z.ident.id });
        wipe(WakeKey::Pends { task: z.ident.id });
        z.ident.team.prune_tasks();
        let _pruned = crate::basis::room::scheduler::roster::prune_dead();
        drop(z);
        tally::exit();
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
        task.signs.clear(task.ident.id);
        task.administration.clear();
    }
    if let Some(hooks) = HOOKS.get() {
        for h in hooks.iter() {
            h(task);
        }
    }
}
