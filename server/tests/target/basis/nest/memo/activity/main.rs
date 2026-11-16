#![no_std]
#![no_main]
mod cases;
mod reservation;
#[allow(dead_code)]
mod support;
#[allow(dead_code)]
mod stack;
use core::fmt::Write;
use kernel_api::{
    abi::{
        call::{room::RoomFail, share_view::ShareFail},
        memory::*,
    },
    address::*,
    model::{
        cell::*,
        identity::*,
        memory::activity::{self as sampling, Evidence, Sample, SamplePage, Validity},
        nest::*,
        pend::*,
        unit::ExitScope,
        wait::Wait,
    },
};
use kernel_client::call::{pend, room, share_view, sign, unit};
use support::*;
core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail run"
);
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL activity {info}");
    exit(1)
}
fn exit(n: u64) -> ! {
    unit::exit(n, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
fn pause() {
    match room::sleep_until(room::clock() + 1_000_000) {
        Ok(()) => {}
        Err(e) => {
            assert_eq!(e.source, RoomFail::Busy);
            room::yield_();
        }
    }
}
fn derive(m: MemoPosition, target: AccordTarget) -> Result<PendId, ShareFail> {
    let source = pend_info(m.token());
    if !matches!(source.target,Target::Nest(id) if id==m.nest())
        || !matches!(source.limits.scope,PendScope::Nest(NestScope::Span {id,..}) if id==m.span())
    {
        return Err(ShareFail::Denied);
    }
    let request = AccordRequest {
        src: m.token(),
        target,
        limits: sampling::limits(m.page()).unwrap(),
        mark: Mark::NONE,
    }
    .to_bytes();
    share_view::derive(license(), VirtAddr::new(request.as_ptr() as usize)).map_err(|e| e.source)
}
fn pair(data: PendId) -> sampling::Pair {
    let end = room::clock() + 8_000_000_000;
    loop {
        match kernel_client::memory::activity::pair(data) {
            Ok(pair) => return pair,
            Err(e) => assert_eq!(e.source, kernel_api::abi::call::pend::PendFail::Busy),
        }
        assert!(room::clock() < end);
        pause();
    }
}
fn observe(m: MemoPosition) -> Result<sampling::Pair, ShareFail> {
    derive(m, AccordTarget::Caller).map(pair)
}
struct Reader {
    data: PendId,
    event: PendId,
    address: VirtAddr,
    mapping: MemoPosition,
}
impl Reader {
    fn open(m: MemoPosition) -> Self {
        Self::open_in(m, space())
    }
    fn open_in(m: MemoPosition, namespace: PendId) -> Self {
        let end = room::clock() + 8_000_000_000;
        let (event, data) = loop {
            match observe(m) {
                Ok(v) => break v,
                Err(ShareFail::Busy) => {
                    assert!(room::clock() < end);
                    pause();
                }
                Err(e) => panic!("observe {e:?}"),
            }
        };
        Self::from_pair((event, data), namespace)
    }
    fn from_pair((event, data): sampling::Pair, namespace: PendId) -> Self {
        let kernel_api::model::pend::Target::View(ViewTarget::Share(view)) = pend_info(data).target
        else {
            panic!("sample View")
        };
        let (mapping, address) = reserve_at(namespace, Placement::lowest(4096).unwrap(), 2);
        mutate(Action::Insert {
            target: mapping,
            expected: memo_status(mapping).generation,
            source: Source::Share {
                token: data,
                view,
                page: match pend_info(data).limits.scope {
                    PendScope::Pages(PendRange::Pages { offset, .. }) => offset,
                    _ => panic!("View window"),
                },
                count: 1,
            },
        });
        publish(mapping);
        Self {
            data,
            event,
            address,
            mapping,
        }
    }
    fn cancel(&self) {
        let end = room::clock() + 8_000_000_000;
        loop {
            match pend::return_(self.event) {
                Ok(()) => break,
                Err(e) if e.source == kernel_api::abi::call::pend::PendFail::Busy => pause(),
                Err(e) => panic!("cancel {e:?}"),
            }
            assert!(room::clock() < end);
        }
        assert!(sign::listen(self.event, Wait::Poll).is_err());
    }
    fn pending(&self) -> (u64, Sample) {
        let end = room::clock() + 8_000_000_000;
        loop {
            match sign::listen(self.event, Wait::Poll) {
                Ok(kernel_api::abi::reply::SignReply::Triggered { sequence }) => {
                    if let Some(s) = unsafe { &*(self.address.get() as *const SamplePage) }.read() {
                        assert_eq!(s.bytes, 4096);
                        return (sequence, s);
                    }
                }
                Ok(_) => {}
                Err(e) => panic!("sample Sign {e:?}"),
            }
            assert!(room::clock() < end, "sample delivery timed out");
            pause();
        }
    }
    fn next(&self) -> Sample {
        let (seq, s) = self.pending();
        sign::consume(self.event, seq).unwrap();
        s
    }
    fn expect(&self, a: Evidence, d: Evidence) -> Sample {
        let end = room::clock() + 8_000_000_000;
        loop {
            let s = self.next();
            assert_eq!(s.validity, Validity::Observed);
            if s.accessed == a && s.dirty == d {
                return s;
            }
            assert!(room::clock() < end, "evidence mismatch {s:?}");
        }
    }
}
#[unsafe(no_mangle)]
extern "C" fn run(at: usize, count: usize) -> ! {
    boot(at, count);
    let slot = create(1);
    mutate(Action::Occupy {
        slot,
        expected: slot_status(slot).version,
    });
    let (memo, address) = reserve(space());
    insert(memo, slot);
    publish(memo);
    const SUPPORTED: bool = option_env!("ACTIVITY_UNSUPPORTED").is_none();
    const STOPPED: bool = option_env!("ACTIVITY_STOPPED").is_some();
    if STOPPED {
        cases::stopped(slot, memo, address);
        exit(0);
    }
    if !SUPPORTED {
        let before = memo_status(memo);
        assert_eq!(observe(memo), Err(ShareFail::Unsupported));
        assert_eq!(memo_status(memo), before);
        unsafe { (address.get() as *mut u64).write_volatile(51) };
        assert_eq!(unsafe { (address.get() as *const u64).read_volatile() }, 51);
        let _ = writeln!(Console, "PASS activity unsupported=1");
        exit(0)
    }
    let one = Reader::open(memo);
    let two = Reader::open(memo);
    one.expect(Evidence::NotObserved, Evidence::NotObserved);
    two.expect(Evidence::NotObserved, Evidence::NotObserved);
    assert_eq!(unsafe { (address.get() as *const u64).read_volatile() }, 0);
    one.expect(Evidence::Observed, Evidence::NotObserved);
    two.expect(Evidence::Observed, Evidence::NotObserved);
    one.expect(Evidence::NotObserved, Evidence::NotObserved);
    let (held, before) = one.pending();
    unsafe {
        (address.get() as *mut u64).write_volatile(0x123456789abcdef0);
    }
    two.expect(Evidence::Observed, Evidence::Observed);
    assert_eq!(
        unsafe { &*(one.address.get() as *const SamplePage) }.read(),
        Some(before)
    );
    sign::consume(one.event, held).unwrap();
    one.expect(Evidence::Observed, Evidence::Observed);
    one.expect(Evidence::NotObserved, Evidence::NotObserved);
    // Current source rights are rechecked, even for already mapped output copies.
    let mut limits = pend_info(memo.token()).limits;
    limits.operations = Operations::Nest(NestRights::INSPECT);
    let request = NarrowRequest {
        token: memo.token(),
        limits,
    }
    .to_bytes();
    pend::narrow(VirtAddr::new(request.as_ptr() as usize)).unwrap();
    assert_eq!(observe(memo), Err(ShareFail::Denied));
    assert!(sign::listen(one.event, Wait::Poll).is_err());
    assert_eq!(pend_info(one.data).status, PendStatus::Dead);
    assert_eq!(pend_info(two.data).status, PendStatus::Dead);
    cases::withdrawn(one.mapping, one.address);
    cases::withdrawn(two.mapping, two.address);
    let _ = writeln!(
        Console,
        "PASS activity read=1 write=1 idle=1 readers=2 unread-retained=1 narrow=1 source-denied=1"
    );
    cases::run();
    exit(0)
}
