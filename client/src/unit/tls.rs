//! 用户 TLS 地基 — 每线程独立 tp 指向的 TLS 块。

use env::MemoryResult;

use core::sync::atomic::{AtomicUsize, Ordering};

use env::{PAGE_SIZE, TaskId, TeamId, ViewId, ViewSelection};
use spinning_top::Spinlock;

#[path = "tls_slot.rs"]
mod slot;
use slot::Slot;

#[path = "tls_token.rs"]
mod token;
pub use token::Tls;
#[path = "tls_cleanup.rs"]
mod cleanup;
pub use cleanup::Cleanup;

pub const TLS_SIZE: usize = PAGE_SIZE;

const SLOTS: usize = 64;
static BLOCKS: Spinlock<[Slot; SLOTS]> = Spinlock::new([Slot::Free; SLOTS]);
static OWED: AtomicUsize = AtomicUsize::new(0);
static PENDING: AtomicUsize = AtomicUsize::new(0);

fn view_of(raw: usize) -> ViewId {
    ViewId::from_bytes(&raw.to_le_bytes()).expect("recorded view id")
}

fn reference_of(raw: usize) -> env::FlatReference {
    env::FlatReference::from_bytes(&raw.to_le_bytes()).expect("recorded reference id")
}

fn claim() -> Option<usize> {
    let owner = crate::memory::current_task();
    let mut slots = BLOCKS.lock();
    for (index, slot) in slots.iter_mut().enumerate() {
        if matches!(slot, Slot::Free) {
            *slot = Slot::Plotting { owner };
            return Some(index);
        }
    }
    None
}

fn abandon(index: usize) {
    BLOCKS.lock()[index] = Slot::Free;
}

fn commit(index: usize, address: usize, view: usize, pending: Option<(TaskId, usize)>) {
    let owner = crate::memory::current_task();
    BLOCKS.lock()[index] = Slot::Live {
        owner,
        address,
        view,
        pending,
    };
}

/// 两个责任都结了才腾格；`Closing`（在飞）永远不腾。
fn finish_if_done(index: usize) {
    let mut slots = BLOCKS.lock();
    if matches!(slots[index], Slot::Done { pending: None, .. }) {
        slots[index] = Slot::Free;
    }
}

/// 解一枚还没解掉的引用——**只在它的所有者那张表里**；失败/不是主人都保留。
fn release_pending(index: usize, pending: Option<(TaskId, usize)>) -> bool {
    let Some((owner, raw)) = pending else {
        return true;
    };
    if crate::memory::current_task() != owner {
        return false;
    }
    let current = {
        let slots = BLOCKS.lock();
        match slots[index] {
            Slot::Live { pending, .. }
            | Slot::Owed { pending, .. }
            | Slot::Closing { pending, .. }
            | Slot::Done { pending, .. } => pending,
            Slot::Free | Slot::Plotting { .. } => None,
        }
    };
    if current != pending {
        return false;
    }
    if crate::memory::release(reference_of(raw)).is_err() {
        return false;
    }
    let mut slots = BLOCKS.lock();
    match &mut slots[index] {
        Slot::Live { pending: held, .. }
        | Slot::Owed { pending: held, .. }
        | Slot::Closing { pending: held, .. }
        | Slot::Done { pending: held, .. } => {
            if *held != pending {
                return false;
            }
            *held = None;
        }
        Slot::Free | Slot::Plotting { .. } => return true,
    }
    drop(slots);
    PENDING.fetch_sub(1, Ordering::AcqRel);
    finish_if_done(index);
    true
}

fn claim_owed(index: usize) -> Option<(usize, Option<(TaskId, usize)>, usize)> {
    let current = crate::memory::current_task();
    let mut slots = BLOCKS.lock();
    match slots[index] {
        Slot::Owed {
            owner,
            address,
            view,
            pending,
        } if owner == current => {
            slots[index] = Slot::Closing { owner, pending };
            Some((view, pending, address))
        }
        _ => None,
    }
}

/// 按地址主张映射责任（归还入口）。
fn claim_close(address: usize) -> Option<(usize, usize, Option<(TaskId, usize)>)> {
    let current = crate::memory::current_task();
    let mut slots = BLOCKS.lock();
    for (index, slot) in slots.iter_mut().enumerate() {
        match *slot {
            Slot::Live {
                owner,
                address: held,
                view,
                pending,
            }
            | Slot::Owed {
                owner,
                address: held,
                view,
                pending,
            } if held == address && owner == current => {
                if matches!(slot, Slot::Live { .. }) {
                    OWED.fetch_add(1, Ordering::AcqRel);
                }
                *slot = Slot::Closing { owner, pending };
                return Some((index, view, pending));
            }
            _ => {}
        }
    }
    None
}

/// 映射已结清 / 没拆成（挂欠，扫描下次再试）。
fn mark_done(index: usize) {
    let mut slots = BLOCKS.lock();
    if let Slot::Closing { owner, pending } = slots[index] {
        slots[index] = Slot::Done { owner, pending };
        OWED.fetch_sub(1, Ordering::AcqRel);
    }
}

fn mark_owed(index: usize, address: usize, view: usize) {
    let mut slots = BLOCKS.lock();
    if let Slot::Closing { owner, pending } = slots[index] {
        slots[index] = Slot::Owed {
            owner,
            address,
            view,
            pending,
        };
    }
}

/// 把映射责任交给一次 `detach`：主张→拆→（结清 | 挂欠）。核心调用在锁外。
fn settle_mapping(
    index: usize,
    view: usize,
    pending: Option<(TaskId, usize)>,
    address: usize,
) -> Cleanup {
    let mapping_done =
        crate::memory::detach(TeamId::new(0), view_of(view), ViewSelection::Whole).is_ok();
    if mapping_done {
        mark_done(index);
    } else {
        mark_owed(index, address, view);
    }
    let reference_done = release_pending(index, pending);
    finish_if_done(index);
    // Report this cleanup attempt, even if another task already reused its slot.
    if mapping_done && reference_done {
        Cleanup::Complete
    } else {
        Cleanup::Deferred
    }
}

/// 顺手扫一遍：欠着的映射、以及还没解掉的引用各试一次。
fn sweep() {
    if OWED.load(Ordering::Acquire) == 0 && PENDING.load(Ordering::Acquire) == 0 {
        return;
    }
    let current = crate::memory::current_task();
    for index in 0..SLOTS {
        let owner = BLOCKS.lock()[index].owner();
        if let Some(owner) = owner
            && owner != current
        {
            // Only an actual terminal observation retires foreign TLS records.
            // Denied alone says nothing about an arbitrary task's liveness.
            if super::observe(owner.into(), env::Wait::POLL).is_ok_and(|reply| reply.is_reaped()) {
                reap_task(owner);
            }
            continue;
        }
        let (pending, done, owed) = {
            let slots = BLOCKS.lock();
            match slots[index] {
                Slot::Live { pending, .. } => (pending, false, false),
                Slot::Done { pending, .. } => (pending, true, false),
                Slot::Owed { .. } => (None, false, true),
                _ => (None, false, false),
            }
        };
        if owed {
            if let Some((view, pending, address)) = claim_owed(index) {
                settle_mapping(index, view, pending, address);
            }
            continue;
        }
        release_pending(index, pending);
        if done {
            finish_if_done(index);
        }
    }
}

pub(super) fn reap_task(task: TaskId) {
    let mut slots = BLOCKS.lock();
    for slot in slots.iter_mut() {
        if slot.owner() != Some(task) {
            continue;
        }
        if matches!(slot, Slot::Owed { .. } | Slot::Closing { .. }) {
            OWED.fetch_sub(1, Ordering::AcqRel);
        }
        if matches!(
            slot,
            Slot::Live {
                pending: Some(_),
                ..
            } | Slot::Owed {
                pending: Some(_),
                ..
            } | Slot::Closing {
                pending: Some(_),
                ..
            } | Slot::Done {
                pending: Some(_),
                ..
            }
        ) {
            PENDING.fetch_sub(1, Ordering::AcqRel);
        }
        *slot = Slot::Free;
    }
}

#[cfg(target_arch = "riscv64")]
#[inline]
pub fn base() -> usize {
    let tp: usize;
    // SAFETY: 纯读 tp，无副作用。
    unsafe {
        core::arch::asm!("mv {0}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
    }
    tp
}

/// 本线程的 TLS 块（一整页）——错类型是 Memory 域的词汇（`OoM` / `NoRegion`）。
///
/// 私有（`Caller`）控制：这一段只归发起它的那个线程。
fn allocate() -> MemoryResult<Tls> {
    sweep();
    // 表满就不取映射：不会出现“映射成功却没有槽记身份”。
    let Some(index) = claim() else {
        return Err(env::make_fail(env::MemoryFail::OoM));
    };
    let placed = crate::memory::allocate_raw(TLS_SIZE);
    let (address, view, reference) = match placed {
        Ok(placed) => placed,
        Err(error) => {
            abandon(index);
            return Err(error);
        }
    };
    // 关系已落地；这份引用现在要解掉——解不掉就记进这一格（带所有者）重试。
    let pending = match crate::memory::release(reference) {
        Ok(()) => None,
        Err(_) => {
            PENDING.fetch_add(1, Ordering::AcqRel);
            Some((crate::memory::current_task(), reference.get()))
        }
    };
    commit(index, address, view.get(), pending);
    Ok(Tls::new(index))
}

/// 显式归还本线程的 TLS 块，可在正常退场前调用。
///
/// 归还按**准确的关系身份**执行（登记时记下的那个 `ViewId`），不是按地址猜所有权；
/// 同步未完成的失败保留身份，下一次再试，不当成功丢掉句柄。
///
/// # Safety
/// The token must describe this task's installed TLS. All TLS-dependent code
/// must have finished; no code may use TLS again before a terminal transfer.
pub unsafe fn retire(token: Tls) -> Cleanup {
    let index = token.index();
    let address = match BLOCKS.lock()[index] {
        Slot::Live { owner, address, .. } if owner == crate::memory::current_task() => address,
        _ => panic!("TLS token no longer owns a live slot"),
    };
    assert_eq!(base(), address, "retiring another TLS installation");
    let (held_index, raw, pending) = claim_close(address).expect("live TLS slot");
    assert_eq!(held_index, index);
    // No operation after this point requires TLS, including the terminal trap.
    unsafe {
        install(0);
    }
    settle_mapping(index, raw, pending, address)
}

/// Installs the current task's TLS only after all fallible setup has succeeded.
///
/// # Safety
/// Call once in a fresh task before any code that requires TLS. The task must
/// have no valid TLS installation. Keep the token in this task if explicitly
/// retiring TLS, and retire only after all TLS-dependent code has finished.
/// An adapter may instead retain the Caller mapping through the existing Kernel
/// terminal path; native task termination then owns mapping cleanup. Retiring
/// local bookkeeping still requires a native terminal observation. The existing
/// sweeper runs only while cleanup is owed or a reference release is pending;
/// it does not guarantee reclaiming a retained Live slot, which may remain until
/// the address space ends. Dropping the token alone neither clears tp nor
/// discharges the table's responsibility.
pub unsafe fn initialize() -> MemoryResult<Tls> {
    if base() != 0 {
        return Err(env::make_fail(env::MemoryFail::Busy));
    }
    let token = allocate()?;
    let address = match BLOCKS.lock()[token.index()] {
        Slot::Live { address, .. } => address,
        _ => unreachable!("committed TLS slot"),
    };
    unsafe {
        install(address);
    }
    Ok(token)
}

#[cfg(target_arch = "riscv64")]
unsafe fn install(address: usize) {
    unsafe {
        core::arch::asm!("mv tp, {}", in(reg) address, options(nomem, nostack, preserves_flags));
    }
}

#[cfg(not(target_arch = "riscv64"))]
fn base() -> usize {
    0
}

#[cfg(not(target_arch = "riscv64"))]
unsafe fn install(_address: usize) {
    unimplemented!("TLS installation requires riscv64");
}
