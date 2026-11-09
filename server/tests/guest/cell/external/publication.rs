use super::*;
use core::fmt::Write;
use kernel_api::{
    abi::call::sign::{SignEventFail, SignReply},
    model::{nest::NestKind, port::PortInfo},
};
use kernel_client::call::{pend, port, share_view};
pub(in super::super) fn run(at: usize, count: usize) {
    let (bar_token, port_token) = roots(at, count);
    let (bar, _) = share_view::open(bar_token).unwrap();
    let mut description = [0; PortInfo::WIDTH];
    port::inspect(port_token, VirtAddr::new(description.as_mut_ptr() as usize)).unwrap();
    let description = PortInfo::from_bytes(&description).unwrap();
    assert_eq!(description.requester, 8);
    assert_eq!(description.window_size, 1 << 21);
    assert!(share_view::open(port_token).is_err());
    let mut out = [0; PortInfo::WIDTH];
    assert!(port::inspect(bar_token, VirtAddr::new(out.as_mut_ptr() as usize)).is_err());
    let root = nest::create(crate::license::get(), NestKind::External).unwrap();
    assert_eq!(context(root).phase, NestPhase::Unbound);
    assert!(nest::attach(root, bar_token).is_err());
    assert!(nest::attach(root, PendId::mint(usize::MAX - 10)).is_err());
    nest::attach(root, port_token).unwrap();
    assert!(nest::attach(root, port_token).is_err());
    let next = nest::create(crate::license::get(), NestKind::External).unwrap();
    assert!(nest::attach(next, port_token).is_err());
    assert_eq!(context(next).phase, NestPhase::Unbound);
    let a = super::super::allocate(1);
    let b = super::super::allocate(1);
    let (cpu_a, address_a) = super::super::attach(a, 1);
    let (cpu_b, address_b) = super::super::attach(b, 1);
    super::super::write(address_a, 0, 0x5a5a5a5a);
    super::super::write(address_b, 0, 0xa5a5a5a5);
    super::super::retract(cpu_a, 0);
    super::super::retract(cpu_b, 0);
    let first = create(root, a, ADDRESS, false);
    let second = create(root, b, ADDRESS + 4096, true);
    let published = sign::create(crate::license::get()).unwrap();
    // Registration before publication and completion before listening use the
    // same retained generation predicate.
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
    transfer(bar, ADDRESS, false); // EDU now contains first Page's bytes.
    transfer(bar, ADDRESS + 4096, true);
    assert_eq!(context(root).fault_sequence, 0);
    // Keep a delayed request outstanding while retracting only the first Memo.
    start(bar, ADDRESS, false);
    cell::retract(super::super::ptr(&current(first).to_bytes())).unwrap();
    let retired = sign::create(crate::license::get()).unwrap();
    sign::attach(retired, source(first, false), current(first).generation).unwrap();
    event(retired, source(first, false));
    assert_eq!(read(bar, 0x98) & 1, 0); // platform proof waited out the queued callback
    assert!(!super::super::info(first).external);
    assert_eq!(super::super::info(second).state, CellState::Published);
    assert!(super::super::info(second).external);
    let late_faults = context(root).fault_sequence;
    assert!(late_faults > 0); // the delayed read reached the invalidated leaf
    transfer(bar, ADDRESS + 4096, true); // the other leaf remains usable
    assert_eq!(context(root).fault_address, ADDRESS as u64);
    transfer(bar, ADDRESS, true); // the withdrawn address is denied
    let until = room::clock() + 2_000_000_000;
    while context(root).fault_sequence == late_faults {
        assert!(room::clock() < until);
        room::yield_();
    }
    let faults = sign::create(crate::license::get()).unwrap();
    sign::attach(faults, SignSource::NestFault { token: root }, 0).unwrap();
    event(faults, SignSource::NestFault { token: root });
    let old_faults = context(root).fault_sequence;
    assert_eq!(context(root).fault_cause, 13); // oldest fault remains the delayed read
    // Reassociate a different Page at the exact retired address. Old listeners
    // must report Gone instead of observing this publication's completion.
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
    // Moving the Slot does not implicitly release its old CPU Memo's freeze.
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
    // FQ records can be consumed after the replacement publication. They stay
    // on this binding; the new Memo never inherits a guessed fault generation.
    assert!(context(root).fault_sequence >= old_faults);
    assert_eq!(context(root).fault_address, ADDRESS as u64);
    assert_eq!(super::super::info(first).fault_cause, 0);
    assert_eq!(super::super::info(second).fault_cause, 0);
    retract(first);
    retract(second);
    // Ready Memos retain the explicit admission freeze. Reassociation releases
    // each external Memo's freeze before the CPU writer is admitted again.
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
    let closing = sign::create(crate::license::get()).unwrap();
    sign::attach(closing, SignSource::NestClosed { token: root }, 0).unwrap();
    nest::detach(root).unwrap();
    event(closing, SignSource::NestClosed { token: root });
    closed(root);
    assert!(nest::attach(root, port_token).is_err());
    assert!(cell::publish(super::super::ptr(&current(first).to_bytes())).is_err());
    use kernel_api::model::{
        identity::Mark,
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, Operations, PendRange, PortRights,
        },
    };
    let grant = AccordRequest {
        src: port_token,
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Port(PortRights::ATTACH),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::Inherit,
        },
    }
    .to_bytes();
    let port_child = pend::accord(super::super::ptr(&grant)).unwrap();
    nest::attach(next, port_child).unwrap();
    assert_ne!(context(root).id, context(next).id);
    assert_eq!(context(next).fault_sequence, 0);
    let rebound = create(next, a, ADDRESS, false);
    publish(rebound);
    transfer(bar, ADDRESS, false);
    assert_eq!(context(next).fault_sequence, 0);
    let wrong = CellPosition {
        object: CellObject::Memo {
            target: MappingTarget::Io(next),
            mapping: match first.object {
                CellObject::Memo { mapping, .. } => mapping,
                _ => unreachable!(),
            },
        },
        ..current(first)
    };
    let mut bytes = [0; CellInfo::WIDTH];
    assert!(
        cell::inspect(
            super::super::ptr(&wrong.to_bytes()),
            VirtAddr::new(bytes.as_mut_ptr() as usize)
        )
        .is_err()
    );
    // Revocation, with the driver still alive and a hardware write queued,
    // closes admission and retains cleanup until the exact platform proof.
    start(bar, ADDRESS, true);
    let revoked = pend::revoke(crate::lifecycle::self_id(), port_child);
    if let Err(ref error) = revoked {
        assert_eq!(error.source, kernel_api::abi::call::pend::PendFail::Busy);
    }
    closed(next);
    if revoked.is_err() {
        pend::revoke(crate::lifecycle::self_id(), port_child).unwrap();
    }
    assert!(nest::attach(root, port_child).is_err());
    assert!(!super::super::info(rebound).external);
    assert_eq!(read(bar, 0x98) & 1, 0);
    for pos in [first, second, rebound] {
        super::super::delete(pos.object);
    }
    for token in [root, next] {
        nest::delete(token).unwrap();
        pend::return_(token).unwrap();
    }
    for token in [published, after, retired, faults, closing] {
        sign::delete(token).unwrap();
        pend::return_(token).unwrap();
    }
    for cpu in [cpu_a, cpu_b] {
        super::super::delete(super::super::raw_memo(cpu, 0).object);
    }
    for slot in [a, b, empty, replacement] {
        super::super::delete(CellObject::Slot(slot));
    }
    super::levels::run(bar, port_token);
    share_view::shut(bar_token).unwrap();
    let _ = writeln!(
        super::super::Console,
        "PASS cell-io external=1 readonly=1 fault=1 fenced=1 ownership=1 multi=2 repeat=1 rebind=1 events=1 revoke=1"
    );
}
