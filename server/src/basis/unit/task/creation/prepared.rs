use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use crate::basis::nest::NestError;
use crate::basis::pend::Table;
use crate::basis::room::scheduler;
use crate::basis::unit::anchor::Anchor;
use crate::basis::unit::lifecycle::tally;
use crate::basis::unit::task::Origin;
use crate::basis::unit::team::MAX_MEMBERS;
use crate::basis::unit::team::Member;
use crate::metal::sync::SpinLock;

use super::super::Task;
use super::super::TaskIdent;
use super::super::TaskState;
use super::super::TaskWeak;

/// 已装配好、尚未公开的任务：身份、生命周期线、状态机，与那块等着写入的外壳内存。
pub(crate) struct PreparedTask {
    ident: Arc<TaskIdent>,
    life: Arc<Anchor>,
    state: Arc<SpinLock<TaskState>>,
    slot: Option<Arc<MaybeUninit<Task>>>,
}

impl PreparedTask {
    pub(crate) fn new(
        ident: Arc<TaskIdent>,
        life: Arc<Anchor>,
        state: Arc<SpinLock<TaskState>>,
        slot: Arc<MaybeUninit<Task>>,
    ) -> PreparedTask {
        PreparedTask {
            ident,
            life,
            state,
            slot: Some(slot),
        }
    }

    /// 把这一份预备公开成团队里的一个成员：先按预留容量落座，再写上外壳、挂上
    /// 团队表与暂扣表。`commit` 是公开那一刻要回调的动作。
    pub(crate) fn publish(&mut self, commit: impl FnOnce()) -> Result<Arc<Task>, NestError> {
        let team = self.ident.team.clone();
        let mut receipts = team.receipts()?;
        let controller = self.ident.controller;
        if controller.get() != 0 && !receipts.iter().any(|receipt| receipt.owner == controller) {
            let mut roots = Vec::new();
            roots.try_reserve(1).map_err(|_| NestError::OutOfMemory)?;
            roots.push(team.id);
            receipts
                .try_reserve(1)
                .map_err(|_| NestError::OutOfMemory)?;
            receipts.push(crate::basis::unit::team::Receipt {
                owner: controller,
                roots,
            });
        }
        let member = Arc::try_new(Member {
            id: self.ident.id,
            team: team.id,
            controller: self.ident.controller,
            creator: team.creator(),
            sire: team
                .sire()
                .unwrap_or(kernel_api::model::identity::TaskId::new(0)),
            reclaimed: core::sync::atomic::AtomicBool::new(false),
            task: TaskWeak::empty(),
            node: Arc::downgrade(&team.node),
            state: self.state.clone(),
            life: self.life.clone(),
            receipts: SpinLock::new(receipts),
            result: SpinLock::new(None),
        })
        .map_err(|_| NestError::OutOfMemory)?;
        scheduler::roster::reserve_publication().map_err(|_| NestError::OutOfMemory)?;
        // 下面的提交段在 `NoAllocation` 下并持几把锁：这里自己四把（提交锁、成员表、
        // 暂扣表、名册），回调里还会取它自己那几把。依赖栈容量按「自己四把 + 四把
        // 余量」先在这里备好：提交段里惰性扩容就是一次分配，回调再取锁就只剩溢出。
        crate::metal::sync::reserve_depend(8).map_err(|_| NestError::OutOfMemory)?;
        {
            let mut tasks = team.node.tasks.lock();
            if tasks.len() >= MAX_MEMBERS {
                return Err(NestError::OutOfMemory);
            }
            let mut held = team.tasks.held.lock();
            tasks.try_reserve(1).map_err(|_| NestError::OutOfMemory)?;
            let held_extra = tasks.len() + 1 - held.len();
            held.try_reserve(held_extra)
                .map_err(|_| NestError::OutOfMemory)?;
        }
        let _commit = crate::basis::commit();
        if !team.node.admitting() {
            return Err(NestError::NoRegion);
        }
        let mut tasks = team.node.tasks.lock();
        let mut held = team.tasks.held.lock();
        if tasks.len() >= MAX_MEMBERS
            || tasks.len() == tasks.capacity()
            || held.capacity() < tasks.len() + 1
        {
            return Err(NestError::OutOfMemory);
        }
        let (task, ()) = scheduler::roster::publish(|| {
            commit();
            assert!(team.ready(), "publish: constructing team");
            let mut slot = self.slot.take().expect("prepare slot");
            Arc::get_mut(&mut slot)
                .expect("unique prepare slot")
                .write(Task {
                    #[cfg(feature = "latency")]
                    ready_stamp: core::sync::atomic::AtomicU64::new(0),
                    ident: self.ident.clone(),
                    life: self.life.clone(),
                    state: self.state.clone(),
                    pend: Table::new(),
                    administration: crate::basis::unit::control::Table::new(),
                    heir: SpinLock::new(Vec::new()),
                    controls: SpinLock::new(Vec::new()),
                    extensions: SpinLock::new(
                        crate::metal::switcher::context::extension::ExtendedContext::new(),
                    ),
                });
            let task = unsafe { slot.assume_init() };
            let mut member = member;
            Arc::get_mut(&mut member).expect("private member").task =
                TaskWeak::stored(Arc::downgrade(&task), Origin::TeamTasks);
            let at = tasks.partition_point(|other| other.id.get() < member.id.get());
            tasks.insert(at, member);
            held.push(task.clone());
            tally::push();
            (task, ())
        })
        .map_err(|_| NestError::OutOfMemory)?;

        drop(held);
        drop(tasks);

        Ok(task)
    }
}
