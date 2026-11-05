//! 域内任务创建与结果回收：spawn、try_spawn 与 Join。
//! 生成用 `Spawn`/`Embark`，结果经共享空间的 `Completion` 槽交回（不占权限表）。

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use env::{TaskId, TeamId, UnitResult, Wait};

use super::tls;

#[path = "task/completion.rs"]
mod completion;
#[path = "task/detached.rs"]
mod detached;
#[path = "task/join.rs"]
mod join;
#[path = "task/send_slot.rs"]
mod send_slot;
use crate::room::{wait, wake};
use crate::unit as env_task;
pub use completion::Completion;
use detached::Detached;
pub use join::Join;
use send_slot::SendSlot;

/// 子任务已完工（result 可取）。
const DONE: usize = 1;
/// 父方已弃权（不再取 result）。
const LEFT: usize = 2;

static DETACHED: crate::lock::Lock<Vec<Detached>> = crate::lock::Lock::new(Vec::new());
unsafe fn release<T>(slot: usize) {
    unsafe {
        drop(Box::from_raw(slot as *mut Completion<T>));
    }
}
// Private to tasks minted by this runtime in its own Team: observation authority
// cannot be revoked. Denied here means the terminal member has been pruned,
// so it permits freeing its heap slot, never inventing a successful T or reason.
fn settle_owned(task: TaskId) -> bool {
    let done = match env_task::join_task(task, Wait::POLL) {
        Ok(done) => done,
        Err(error) => error.source == env::UnitFail::Denied,
    };
    if done {
        tls::reap_task(task);
    }
    done
}
fn sweep() {
    let mut pending = DETACHED.with(core::mem::take);
    let mut at = 0;
    while at < pending.len() {
        if settle_owned(pending[at].task) {
            let item = pending.swap_remove(at);
            // Destructors run outside the registry lock and can drop other Joins.
            unsafe {
                (item.release)(item.slot);
            }
        } else {
            at += 1;
        }
    }
    if !pending.is_empty() {
        DETACHED.with(|list| list.append(&mut pending));
    }
}
/// 域内产线程跑一个闭包，返回 `Join<T>` 取回结果。
///
/// `spawn` 恒产 `Held`，故此处紧接着 `embark`——域内线程无需跨域授权序，
/// 数据经**共享空间**的 `Completion` 槽传递（不占权限表）。
///
/// **生成失败即 panic**（`expect`）：本函数是「产线程」这条语义的便捷面，
/// 调用点当它不会失败。要**观测**失败（压测/自检要分辨「没生出来」与
/// 「生出来且回收干净」）用 [`try_spawn`]——两者的成功路径是同一份装配。
///
/// `T: Send` 是**结果也要过线程边界**这句话：句柄（`PieToken`）是**表**的、
/// 标着 `!Send`，故"生出来的线程把一枚号交回来"在这里就是**编译不过**——
/// 跨线程要交的从来不是号，是 `ship` 出去的那一枚副本（`env::wire::handle`）。
pub fn spawn<F, T>(f: F) -> Join<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send,
{
    try_spawn(f).expect("task spawn failed")
}

/// [`spawn`] 的可失败版：把 `Spawn` / `Embark` 的错误原样交回调用方。
///
/// Spawn failure releases both local allocations. A failed Embark terminates
/// the unpublished closure task before reclaiming its startup argument.
pub fn try_spawn<F, T>(f: F) -> UnitResult<Join<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send,
{
    sweep();
    let slot = Box::into_raw(Box::new(Completion::new()));
    let send_slot = SendSlot(slot);
    // 先 done 后 wake：done Release 发表于 ecall 之前；唤醒后 Acquire 重查必见真值。
    let inner: Box<dyn FnOnce() + Send> = Box::new(move || {
        let r = f();
        let slot_ptr = send_slot.0;
        unsafe { send_slot.store_result(r) }
        let _ = wake(slot_ptr as usize);
    });
    let holder: Box<Box<dyn FnOnce() + Send>> = Box::new(inner);
    let ptr = Box::into_raw(holder) as usize;
    let task_id = match super::spawn(
        TeamId::new(0),
        (trampoline as extern "C" fn(usize) -> !) as usize,
        &[ptr],
        0,
    ) {
        Ok(task) => task,
        Err(error) => {
            unsafe {
                drop(Box::from_raw(ptr as *mut Box<dyn FnOnce() + Send>));
                drop(Box::from_raw(slot));
            }
            return Err(error);
        }
    };
    // Held guarantees that the child cannot dereference the startup pointer yet.
    if let Err(error) = env_task::embark_task(task_id) {
        let _ = env_task::slay_task(task_id);
        if env_task::join_task(task_id, Wait::Forever)
            .unwrap_or_else(|error| error.source == env::UnitFail::Denied)
        {
            unsafe {
                drop(Box::from_raw(ptr as *mut Box<dyn FnOnce() + Send>));
                drop(Box::from_raw(slot));
            }
        }
        return Err(error);
    }
    Ok(Join { slot, id: task_id })
}

/// 当前 task id（`UnitCall::SelfId`）。无上下文 → `TaskId(0)`。
///
/// 返回句柄而非裸数：它是本任务身份、要喂给 `accord`/`revoke` 这类权柄操作，
/// 化回 `usize` 只会让调用点不得不再包一次。
///
/// **不返 `Result`**：内核那一格恒写 id（无上下文也是 0），没有失败支
/// （生成的入口标了 `#[infallible]`）。
pub fn self_id() -> TaskId {
    env_task::self_id()
}

#[unsafe(no_mangle)]
pub extern "C" fn trampoline(arg: usize) -> ! {
    // a0 = 启动参数区 VA（`Spawn` 的 args 写在栈顶）；args[0] = 闭包装箱薄指针。
    // 必须在任何调用（tls::alloc）之前读——a0 是 caller-saved。
    let ptr = unsafe { core::ptr::read_volatile(arg as *const usize) };
    let tls = unsafe { tls::initialize() }.expect("tls alloc failed");
    let holder: Box<Box<dyn FnOnce() + Send>> =
        unsafe { Box::from_raw(ptr as *mut Box<dyn FnOnce() + Send>) };
    holder();
    sweep();
    // 正常退出提前 Detach；异常退出由内核结清 Caller 映射，终态回收本地 TLS 登记。
    unsafe {
        tls::retire(tls);
    }
    crate::room::reap(env::EXIT_OK, None)
}
