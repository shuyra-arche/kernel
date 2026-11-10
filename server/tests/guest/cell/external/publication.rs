use super::*;
use core::fmt::Write;
use kernel_api::{
    abi::call::{
        nest::NestFail,
        pend::PendFail,
        sign::{SignEventFail, SignReply},
    },
    model::{
        pend::NarrowRequest,
        unit::{ControlId, ProgramKind, UnitCreate},
    },
};
use kernel_client::call::{share_view, unit};
pub(in super::super) fn run(at: usize, count: usize) {
    let (bar_token, root) = roots(at, count);
    let identity = context(root).id;
    let (bar, _) = share_view::open(bar_token).unwrap();
    assert!(share_view::open(root).is_err());
    let mut out = [0; NestInfo::WIDTH];
    for invalid in [
        bar_token,
        crate::license::get(),
        PendId::mint(usize::MAX - 10),
    ] {
        assert!(nest::inspect(invalid, VirtAddr::new(out.as_mut_ptr() as usize)).is_err());
    }
    assert!(nest::create(root).is_err());
    let team = UnitCreate::Team {
        member_exit: false,
        nest: Some(root),
        kind: ProgramKind::User,
        guard: ControlId::NONE,
    }
    .to_bytes();
    assert!(unit::create(super::super::ptr(&team)).is_err());
    // Binding retirement is retained, but it must not freeze root attenuation.
    let narrow = NarrowRequest {
        token: root,
        limits: Limits {
            operations: Operations::Nest(NestRights::all() & !NestRights::ATTACH),
            distributable: Distributable::Yes,
            ..limits()
        },
    }
    .to_bytes();
    pend::narrow(super::super::ptr(&narrow)).unwrap();
    let delete = AccordRequest {
        src: root,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Nest(NestRights::DELETE),
            ..limits()
        },
    }
    .to_bytes();
    let delegated_delete = pend::accord(super::super::ptr(&delete)).unwrap();
    assert_eq!(
        nest::delete(delegated_delete).unwrap_err().source,
        NestFail::Denied
    );
    assert_eq!(context(root).phase, NestPhase::Bound);
    returned(delegated_delete);

    let first_branch = branch(root);
    let second_branch = branch(root);
    assert_eq!(context(first_branch).id, identity);
    assert_eq!(context(second_branch).id, identity);
    assert_eq!(
        nest::delete(first_branch).unwrap_err().source,
        NestFail::Denied
    );
    let forbidden = AccordRequest {
        src: first_branch,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: limits(),
    }
    .to_bytes();
    assert_eq!(
        pend::accord(super::super::ptr(&forbidden))
            .unwrap_err()
            .source,
        PendFail::Denied
    );
    let a = super::super::allocate(1);
    let b = super::super::allocate(1);
    let (cpu_a, address_a) = super::super::attach(a, 1);
    let (cpu_b, address_b) = super::super::attach(b, 1);
    super::super::write(address_a, 0, 0x5a5a5a5a);
    super::super::write(address_b, 0, 0xa5a5a5a5);
    super::super::retract(cpu_a, 0);
    super::super::retract(cpu_b, 0);
    let first = create(first_branch, a, ADDRESS, false);
    let second = create(second_branch, b, ADDRESS + 4096, true);
    // An Internal Nest and a data View cannot name this external Memo.
    let internal = nest::create(crate::license::get()).unwrap();
    assert!(observed(alias(first, internal)).is_none());
    assert!(observed(alias(first, bar_token)).is_none());
    nest::delete(internal).unwrap();
    returned(internal);
    let published = sign::create(crate::license::get()).unwrap();
    sign::attach(
        published,
        source(first, true),
        current(first).generation + 1,
    )
    .unwrap();
    publish(first);
    publish(second);
    event(published, source(first, true));
    let after = sign::create(crate::license::get()).unwrap();
    sign::attach(after, source(second, true), current(second).generation).unwrap();
    event(after, source(second, true));
    transfer(bar, ADDRESS, false);
    transfer(bar, ADDRESS + 4096, true);
    assert_eq!(context(root).fault_sequence, 0);
    // A delayed device request must finish before this one address is reusable.
    start(bar, ADDRESS, false);
    cell::retract(super::super::ptr(&current(first).to_bytes())).unwrap();
    let retired = sign::create(crate::license::get()).unwrap();
    sign::attach(retired, source(first, false), current(first).generation).unwrap();
    event(retired, source(first, false));
    assert_eq!(read(bar, 0x98) & 1, 0);
    assert!(!super::super::info(first).external);
    assert_eq!(super::super::info(second).state, CellState::Published);
    assert!(super::super::info(second).external);
    let late_faults = context(root).fault_sequence;
    assert!(late_faults > 0);
    transfer(bar, ADDRESS + 4096, true);
    assert_eq!(context(root).fault_address, ADDRESS as u64);
    transfer(bar, ADDRESS, true);
    let until = room::clock() + 2_000_000_000;
    while context(root).fault_sequence == late_faults {
        assert!(room::clock() < until);
        room::yield_();
    }
    let faults = sign::create(crate::license::get()).unwrap();
    sign::attach(faults, SignSource::NestFault { token: root }, 0).unwrap();
    event(faults, SignSource::NestFault { token: root });
    let old_faults = context(root).fault_sequence;
    assert_eq!(context(root).fault_cause, 13);
    // Reassociate a different Page at the retired address. Older listeners must
    // report Gone, not success for a later publication at the same IOVA.
    let empty = super::super::allocate(1);
    let replacement = super::super::allocate(1);
    let (cpu_r, address_r) = super::super::attach(replacement, 1);
    super::super::write(address_r, 0, 0xc3c3c3c3);
    super::super::retract(cpu_r, 0);
    super::super::handoff(current(first), super::super::slot(empty, 0), true);
    super::super::handoff(
        super::super::slot(replacement, 0),
        super::super::slot(a, 0),
        false,
    );
    super::super::delete(super::super::raw_memo(cpu_r, 0).object);
    super::super::handoff(super::super::slot(a, 0), current(first), false);
    let stale = current(first);
    publish(first);
    assert!(cell::retract(super::super::ptr(&stale.to_bytes())).is_err());
    assert_eq!(
        sign::listen(published, Wait::Poll).unwrap(),
        SignReply::Source {
            source: source(first, true),
            fail: Some(SignEventFail::Gone)
        }
    );
    assert_eq!(
        sign::listen(retired, Wait::Poll).unwrap(),
        SignReply::Source {
            source: source(first, false),
            fail: Some(SignEventFail::Gone)
        }
    );
    transfer(bar, ADDRESS, false);
    transfer(bar, ADDRESS + 4096, true);
    assert!(context(root).fault_sequence >= old_faults);
    assert_eq!(context(root).fault_address, ADDRESS as u64);
    assert_eq!(super::super::info(first).fault_cause, 0);
    assert_eq!(super::super::info(second).fault_cause, 0);
    retract(first);
    retract(second);
    super::super::handoff(super::super::slot(a, 0), current(first), false);
    super::super::handoff(super::super::slot(b, 0), current(second), false);
    super::super::handoff(
        super::super::slot(a, 0),
        super::super::memo(cpu_a, 0),
        false,
    );
    super::super::publish(cpu_a, 0);
    super::super::publish(cpu_b, 0);
    assert_eq!(super::super::read(address_a, 0), 0xc3c3c3c3);
    assert_eq!(super::super::read(address_b, 0), 0xc3c3c3c3);
    super::super::retract(cpu_a, 0);
    super::super::retract(cpu_b, 0);
    // Revoking one active branch must retain its own delayed access, while the
    // sibling leaf and the persistent root remain usable.
    publish(first);
    publish(second);
    let old_mapping = alias(current(first), root);
    start(bar, ADDRESS, true);
    let until = room::clock() + 2_000_000_000;
    loop {
        match pend::revoke(crate::lifecycle::self_id(), first_branch) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error.source, PendFail::Busy);
                assert!(room::clock() < until, "branch retirement stalled");
                room::yield_();
            }
        }
    }
    assert_eq!(read(bar, 0x98) & 1, 0);
    assert_eq!(context(root).id, identity);
    assert_eq!(context(root).phase, NestPhase::Bound);
    assert!(observed(old_mapping).is_none());
    assert_eq!(super::super::info(second).state, CellState::Published);
    transfer(bar, ADDRESS + 4096, true);
    let next = branch(root);
    assert_eq!(context(next).id, identity);
    let fresh = create(next, a, ADDRESS, false);
    publish(fresh);
    transfer(bar, ADDRESS, false);
    assert!(observed(alias(old_mapping, next)).is_none());
    assert_eq!(super::super::info(fresh).fault_cause, 0);
    retract(fresh);
    retract(second);
    super::super::delete(fresh.object);
    super::super::delete(second.object);
    returned(next);
    returned(second_branch);
    // Returning a branch that never published must also remove its reservation.
    let unused = branch(root);
    let position = create(unused, empty, ADDRESS + 8192, false);
    returned(unused);
    assert!(observed(alias(position, root)).is_none());
    assert_eq!(context(root).id, identity);
    assert_eq!(context(root).phase, NestPhase::Bound);
    for token in [published, after, retired, faults] {
        sign::delete(token).unwrap();
        returned(token);
    }
    super::levels::run(bar, root);
    // Whole-Nest deletion is separate from all of the successful handovers.
    let final_position = create(root, a, ADDRESS, false);
    publish(final_position);
    start(bar, ADDRESS, false);
    let closing = sign::create(crate::license::get()).unwrap();
    sign::attach(closing, SignSource::NestClosed { token: root }, 0).unwrap();
    assert_eq!(nest::delete(root).unwrap_err().source, NestFail::Busy);
    assert!(reserve(root, b, ADDRESS + 4096, true).is_err());
    event(closing, SignSource::NestClosed { token: root });
    closed(root);
    nest::delete(root).unwrap();
    assert!(nest::inspect(root, VirtAddr::new(out.as_mut_ptr() as usize)).is_err());
    returned(root);
    sign::delete(closing).unwrap();
    returned(closing);
    for cpu in [cpu_a, cpu_b] {
        super::super::delete(super::super::raw_memo(cpu, 0).object);
    }
    for slot in [a, b, empty, replacement] {
        super::super::delete(CellObject::Slot(slot));
    }
    share_view::shut(bar_token).unwrap();
    let _ = writeln!(
        super::super::Console,
        "PASS cell-io external=1 readonly=1 fault=1 fenced=1 ownership=1 multi=2 repeat=1 handover=1 same-nest=1 events=1 revoke=1 orphan=1"
    );
}
