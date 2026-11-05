#![cfg(test)]
#![allow(dead_code)]

use std::sync::Mutex;

#[derive(Default)]
struct Effects {
    task: usize,
    next: usize,
    allocate_fail: bool,
    detach_fail: bool,
    release_fail: bool,
    reaped: bool,
    views: Vec<usize>,
    references: Vec<(usize, usize)>,
    detached: Vec<usize>,
    observed: Vec<usize>,
}

static EFFECTS: Mutex<Effects> = Mutex::new(Effects {
    task: 1,
    next: 0,
    allocate_fail: false,
    detach_fail: false,
    release_fail: false,
    reaped: false,
    views: Vec::new(),
    references: Vec::new(),
    detached: Vec::new(),
    observed: Vec::new(),
});

mod memory {
    pub fn current_task() -> env::TaskId {
        env::TaskId::new(crate::EFFECTS.lock().unwrap().task)
    }
    pub fn allocate_raw(
        size: usize,
    ) -> env::MemoryResult<(usize, env::ViewId, env::FlatReference)> {
        assert_eq!(size, env::PAGE_SIZE);
        let mut effects = crate::EFFECTS.lock().unwrap();
        if effects.allocate_fail {
            return Err(env::make_fail(env::MemoryFail::OoM));
        }
        effects.next += 1;
        let raw = effects.next;
        let owner = effects.task;
        effects.views.push(raw);
        effects.references.push((owner, raw));
        Ok((
            raw * env::PAGE_SIZE,
            env::ViewId::from_bytes(&raw.to_le_bytes()).unwrap(),
            env::FlatReference::from_bytes(&raw.to_le_bytes()).unwrap(),
        ))
    }
    pub fn release(reference: env::FlatReference) -> env::MemoryResult<()> {
        let mut effects = crate::EFFECTS.lock().unwrap();
        if effects.release_fail {
            return Err(env::make_fail(env::MemoryFail::Busy));
        }
        let owner = effects.task;
        let at = effects
            .references
            .iter()
            .position(|entry| *entry == (owner, reference.get()))
            .expect("release uses the recorded owner's reference");
        effects.references.remove(at);
        Ok(())
    }
    pub fn detach(
        team: env::TeamId,
        view: env::ViewId,
        selection: env::ViewSelection,
    ) -> env::MemoryResult<()> {
        assert_eq!(team.get(), 0);
        assert_eq!(selection, env::ViewSelection::Whole);
        let mut effects = crate::EFFECTS.lock().unwrap();
        if effects.detach_fail {
            return Err(env::make_fail(env::MemoryFail::Busy));
        }
        let at = effects
            .views
            .iter()
            .position(|entry| *entry == view.get())
            .expect("exact view identity");
        effects.views.remove(at);
        effects.detached.push(view.get());
        Ok(())
    }
}

fn observe(target: env::UnitTarget, _: env::Wait) -> env::UnitResult<env::JoinReply> {
    let env::UnitTarget::Task(task) = target else {
        panic!("expected task observation")
    };
    let mut effects = EFFECTS.lock().unwrap();
    effects.observed.push(task.get());
    if effects.reaped {
        Ok(env::JoinReply::Reaped(env::TaskExit {
            task,
            cause: env::ExitCause::Fault,
            reason: 3,
        }))
    } else {
        Err(env::make_fail(env::UnitFail::Denied))
    }
}

#[path = "../../src/unit/tls.rs"]
mod tls;

#[test]
fn initialization_and_deferred_retirement_keep_the_unique_slot_responsible() {
    use tls::Cleanup;
    EFFECTS.lock().unwrap().allocate_fail = true;
    assert!(unsafe { tls::initialize() }.is_err());
    assert_eq!(tls::test_state(), (0, 0, 0, 0));
    EFFECTS.lock().unwrap().allocate_fail = false;

    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(tls::test_state(), (1, 0, 0, env::PAGE_SIZE));
    assert_eq!(
        unsafe { tls::initialize() }.err().unwrap().source,
        env::MemoryFail::Busy
    );
    assert_eq!(tls::test_state(), (1, 0, 0, env::PAGE_SIZE));
    EFFECTS.lock().unwrap().detach_fail = true;
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Deferred);
    assert_eq!(tls::test_state(), (1, 1, 0, 0));
    assert_eq!(EFFECTS.lock().unwrap().views, [1]);

    // Retry uses the recorded relation, then establishes a separate new slot.
    EFFECTS.lock().unwrap().detach_fail = false;
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(EFFECTS.lock().unwrap().detached, [1]);
    assert_eq!(tls::test_state(), (1, 0, 0, 2 * env::PAGE_SIZE));
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Complete);
    assert_eq!(tls::test_state(), (0, 0, 0, 0));

    // A failed reference release stays owned by the original task's table slot.
    EFFECTS.lock().unwrap().release_fail = true;
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(tls::test_state(), (1, 0, 1, 3 * env::PAGE_SIZE));
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Deferred);
    assert_eq!(tls::test_state(), (1, 0, 1, 0));
    assert_eq!(EFFECTS.lock().unwrap().references, [(1, 3)]);

    // Foreign Denied neither drops identity nor releases somebody else's reference.
    {
        let mut effects = EFFECTS.lock().unwrap();
        effects.task = 2;
        effects.release_fail = false;
    }
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(tls::test_state(), (2, 0, 1, 4 * env::PAGE_SIZE));
    assert_eq!(EFFECTS.lock().unwrap().references, [(1, 3)]);
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Complete);
    assert_eq!(tls::test_state(), (1, 0, 1, 0));
    assert!(EFFECTS.lock().unwrap().observed.contains(&1));

    // The owner retries its pending reference successfully.
    EFFECTS.lock().unwrap().task = 1;
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(tls::test_state(), (1, 0, 0, 5 * env::PAGE_SIZE));
    assert!(EFFECTS.lock().unwrap().references.is_empty());
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Complete);

    // Only a real native terminal receipt permits clearing a foreign slot.
    EFFECTS.lock().unwrap().detach_fail = true;
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Deferred);
    {
        let mut effects = EFFECTS.lock().unwrap();
        effects.task = 2;
        effects.reaped = true;
        effects.detach_fail = false;
        // Model native responsibility for terminal Caller cleanup. Reaped does
        // not claim synchronous physical removal; Server may finish it later.
        effects.views.clear();
    }
    let token = unsafe { tls::initialize() }.unwrap();
    assert_eq!(tls::test_state(), (1, 0, 0, 7 * env::PAGE_SIZE));
    assert_eq!(unsafe { tls::retire(token) }, Cleanup::Complete);
    assert_eq!(tls::test_state(), (0, 0, 0, 0));
}
