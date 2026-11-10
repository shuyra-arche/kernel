use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};
use kernel_api::{
    abi::{
        call::{
            pend::operations::Collect,
            share_view::operations::Open,
            sign::operations::{Consume, Listen, Trigger},
            unit::operations::Exit,
        },
        operation::RegisterCall,
        reply::SignReply,
    },
    address::VirtAddr,
    model::{
        cell::*,
        identity::{Mark, PendId, TeamId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, Operations, PendInfo, PendRange,
            ShareRights,
        },
        sign::SignRights,
        unit::{ControlId, ExitCause, ProgramKind, UnitCreate, UnitQuery, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::{cell, pend, room, share_view, sign, unit};

// This is copied into a separate Team. Keep the instructions position-independent.
core::arch::global_asm!(
    r#"
    .pushsection .text.share_test,"ax"
    .balign 4
    share_reader_start:
        addi sp, sp, -{buffer}
        li a0, 0
        mv a1, sp
        li a2, 4
        li a7, {collect}
        ebreak
        li t0, 4
        bne a0, t0, 9f
        ld s0, 0(sp)
        ld s1, {stride}(sp)
        ld s2, {two_strides}(sp)
        ld s3, {three_strides}(sp)
        addi sp, sp, {buffer}

        mv a0, s0
        li a7, {open}
        ebreak
        bltz a0, 9f
        mv s4, a0
        mv a0, s1
        li a7, {open}
        ebreak
        bltz a0, 9f
        mv s5, a0

        li s6, 0
    2:
        jal s7, 8f
        ld t0, 0(s4)
        fence r, rw
        addi t1, s6, 4
        bne t0, t1, 9f
    3:
        andi t2, s6, 3
        slli t2, t2, 3
        add t2, s4, t2
        ld t3, 8(t2)
        addi s6, s6, 1
        bne t3, s6, 9f
        bltu s6, t1, 3b
        fence rw, w
        sd s6, 0(s5)

        mv a0, s3
        li a7, {trigger}
        ebreak
        bnez a0, 9f
        li t0, 32
        bltu s6, t0, 2b

        # The producer revokes only our data Pend before sending this event.
        jal s7, 8f
        mv a0, s0
        li a7, {open}
        ebreak
        bgez a0, 9f
        ld t0, 0(s4)  # The old mapping must fault.
        j 9f

    8:
        mv a0, s2
        li a1, 2
        li a2, 0
        li a7, {listen}
        ebreak
        li t0, 9
        bne a0, t0, 9f
        mv a0, s2
        li a7, {consume}
        ebreak
        bnez a0, 9f
        jr s7

    9:
        li a0, 1
        li a1, 0
        li a2, 0
        li a3, 0
        li a4, 0
        li a7, {exit}
        ebreak
        j .
    share_reader_end:
    .popsection
    "#,
    buffer = const 4 * PendInfo::SIZE,
    stride = const PendInfo::SIZE,
    two_strides = const 2 * PendInfo::SIZE,
    three_strides = const 3 * PendInfo::SIZE,
    collect = const Collect::SLOT,
    open = const Open::SLOT,
    listen = const Listen::SLOT,
    consume = const Consume::SLOT,
    trigger = const Trigger::SLOT,
    exit = const Exit::SLOT,
);
unsafe extern "C" {
    static share_reader_start: u8;
    static share_reader_end: u8;
}

fn grant(src: PendId, target: AccordTarget, operations: Operations, access: Modifiable) -> PendId {
    pend::accord(crate::ptr(
        &AccordRequest {
            src,
            target,
            mark: Mark::NONE,
            limits: Limits {
                operations,
                modifiable: access,
                distributable: Distributable::No,
                range: PendRange::Inherit,
            },
        }
        .to_bytes(),
    ))
    .unwrap()
}

fn reader_team() -> (TeamId, SlotId, PendId, VirtAddr) {
    let UnitTarget::Team(team) = unit::create(crate::ptr(
        &UnitCreate::Team {
            member_exit: false,
            nest: None,
            kind: ProgramKind::User,
            guard: ControlId::NONE,
        }
        .to_bytes(),
    ))
    .unwrap() else {
        panic!("expected a new Team")
    };
    let code = crate::allocate(1);
    let (staging, address) = crate::attach(code, 1);
    let start = core::ptr::addr_of!(share_reader_start);
    let len = core::ptr::addr_of!(share_reader_end).addr() - start.addr();
    assert!(len <= 4096);
    unsafe { core::ptr::copy_nonoverlapping(start, address.get() as *mut u8, len) };
    crate::retract(staging, 0);
    crate::delete(crate::raw_memo(staging, 0).object);
    let source = share_view::create(crate::license::get(), code, Modifiable::No).unwrap();
    let CreatedCell::Memo {
        token: mapping,
        address,
    } = cell::create(
        PendId::NONE,
        crate::ptr(
            &CellCreate::Memo {
                source: CellSource::Pend {
                    token: source,
                    offset: 0,
                    count: 1,
                },
                destination: MemoDestination::Internal {
                    team,
                    placement: Placement::lowest(4096).unwrap(),
                    access: 10,
                },
            }
            .to_bytes(),
        ),
    )
    .unwrap()
    else {
        panic!("expected an executable Memo")
    };
    let position = CellPosition {
        object: CellObject::Memo(mapping),
        page: 0,
        generation: 2,
    };
    cell::publish(crate::ptr(&position.to_bytes())).unwrap();
    (team, code, source, address)
}

fn trigger(token: PendId) {
    sign::trigger(token).unwrap();
}

fn listen(token: PendId) {
    let SignReply::Triggered { sequence } =
        sign::listen(token, Wait::At(room::clock().saturating_add(5_000_000_000))).unwrap()
    else {
        panic!("reader did not acknowledge the ring")
    };
    sign::consume(token, sequence).unwrap();
}

pub(super) fn run() {
    let (team, code, code_pend, entry) = reader_team();
    let data = crate::allocate(1);
    let cursor = crate::allocate(1);
    let producer = share_view::create(crate::license::get(), data, Modifiable::Yes).unwrap();
    let consumer = share_view::create(crate::license::get(), cursor, Modifiable::Yes).unwrap();
    let (address, _) = share_view::open(producer).unwrap();
    let rights = Operations::Share(ShareRights::OPEN | ShareRights::SHUT);
    let read_cursor = grant(consumer, AccordTarget::Caller, rights, Modifiable::No);
    let (consumed, _) = share_view::open(read_cursor).unwrap();
    let send = sign::create(crate::license::get()).unwrap();
    let ack = sign::create(crate::license::get()).unwrap();

    // The held Task discovers the four handles in their Pend grant order.
    let descriptor = UnitCreate::Task {
        team: Some(team),
        entry,
        args: VirtAddr::new(0),
        count: 0,
        stack: VirtAddr::new(0),
    };
    let UnitTarget::Task(task) = unit::create(crate::ptr(&descriptor.to_bytes())).unwrap() else {
        panic!("expected a reader Task")
    };
    let remote = AccordTarget::Task(task);
    let data_grant = grant(producer, remote, rights, Modifiable::No);
    grant(consumer, remote, rights, Modifiable::Yes);
    grant(
        send,
        remote,
        Operations::Sign(SignRights::LISTEN | SignRights::CONSUME),
        Modifiable::No,
    );
    grant(
        ack,
        remote,
        Operations::Sign(SignRights::TRIGGER),
        Modifiable::No,
    );
    unit::resume(UnitTarget::Task(task)).unwrap();

    let produced = unsafe { AtomicUsize::from_ptr(address.get() as *mut usize) };
    let consumed = unsafe { AtomicUsize::from_ptr(consumed.get() as *mut usize) };
    for base in (0..32).step_by(4) {
        assert_eq!(consumed.load(Ordering::Acquire), base);
        for i in 0..4 {
            crate::write(address, 8 + i * 8, base + i + 1);
        }
        produced.store(base + 4, Ordering::Release);
        trigger(send);
        listen(ack);
    }
    assert_eq!(consumed.load(Ordering::Acquire), 32);

    let deadline = room::clock().saturating_add(5_000_000_000);
    while let Err(error) = pend::revoke(task, data_grant) {
        assert_eq!(error.source, kernel_api::abi::call::pend::PendFail::Busy);
        assert!(
            room::clock() < deadline,
            "reader mapping retirement stalled"
        );
        room::yield_();
    }
    produced.store(33, Ordering::Release);
    assert_eq!(produced.load(Ordering::Relaxed), 33);
    trigger(send);
    assert!(crate::lifecycle::wait_task(task));
    let result = crate::lifecycle::inspect(UnitQuery::Target(UnitTarget::Task(task)));
    assert!(
        matches!(result.exit, Some((ExitCause::Fault, _))),
        "{result:?}"
    );
    crate::lifecycle::delete(task);
    unit::finish(team).unwrap();
    while let Err(error) = unit::delete(UnitTarget::Team(team)) {
        assert_eq!(error.source, kernel_api::abi::call::unit::UnitFail::Busy);
        assert!(room::clock() < deadline, "reader Team retirement stalled");
        room::yield_();
    }
    for endpoint in [send, ack] {
        sign::delete(endpoint).unwrap();
        crate::lifecycle::return_pend(endpoint);
    }
    for token in [read_cursor, consumer, producer, code_pend] {
        crate::lifecycle::return_pend(token);
    }
    for slot in [cursor, data, code] {
        crate::delete(CellObject::Slot(slot));
    }
    writeln!(crate::Console, "PASS share-ring teams=2 wrap=1 revoke=1").unwrap();
}
