//! One bounded request mailbox, independent of the worker's authority ledger.
use super::*;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use kernel_client::call::{room, unit};

enum State {
    Idle = 0,
    Request = 1,
    Reply = 2,
    Observation = 3,
    Stop = 4,
    Error = 5,
    Accord = 6,
    Token = 7,
}
struct Channel {
    leased: AtomicBool,
    state: AtomicU8,
    bytes: [AtomicU8; Request::WIDTH],
}
static CHANNELS: [Channel; 2] = [const {
    Channel {
        leased: AtomicBool::new(false),
        state: AtomicU8::new(State::Idle as u8),
        bytes: [const { AtomicU8::new(0) }; Request::WIDTH],
    }
}; 2];
pub struct Worker(pub TaskId, usize);
impl Worker {
    pub fn new() -> Self {
        let index = CHANNELS
            .iter()
            .position(|c| {
                c.leased
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
            .expect("free worker channel");
        let channel = &CHANNELS[index];
        channel.state.store(State::Idle as u8, Ordering::Release);
        let task = spawn(if index == 0 { entry0 } else { entry1 });
        unit::resume(UnitTarget::Task(task)).unwrap();
        Self(task, index)
    }
    pub fn call(&self, action: Action) -> Result<Response, kernel_client::memory::Error> {
        let deadline = room::clock() + 5_000_000_000;
        loop {
            self.begin(action);
            let response = self.finish()?;
            if matches!(action, Action::Block { .. } | Action::Allow { .. }) && matches!(&response, Response::Reply(r) if r.outcome() == Outcome::Rejected(Failure::Busy)) {
                assert!(room::clock() < deadline);
                room::yield_();
                continue;
            }
            return Ok(response);
        }
    }
    pub fn begin(&self, action: Action) {
        let channel = &CHANNELS[self.1];
        for (out, byte) in channel.bytes.iter().zip(request(action).to_bytes()) {
            out.store(byte, Ordering::Relaxed);
        }
        channel.state.store(State::Request as u8, Ordering::Release);
    }
    pub fn finish(&self) -> Result<Response, kernel_client::memory::Error> {
        let channel = &CHANNELS[self.1];
        let deadline = room::clock() + 3_000_000_000;
        let state = loop {
            let state = channel.state.load(Ordering::Acquire);
            if state == State::Reply as u8
                || state == State::Observation as u8
                || state == State::Error as u8
            {
                break state;
            }
            assert!(room::clock() < deadline, "acceptance worker stalled");
            room::yield_();
        };
        let bytes: [u8; 128] = core::array::from_fn(|i| channel.bytes[i].load(Ordering::Relaxed));
        channel.state.store(State::Idle as u8, Ordering::Release);
        if state == State::Error as u8 {
            Err(kernel_client::memory::Error::Delivery(
                Failure::from_code(i64::from_le_bytes(bytes[..8].try_into().unwrap())).unwrap(),
            ))
        } else if state == State::Reply as u8 {
            Ok(Response::Reply(Reply::from_bytes(&bytes).unwrap()))
        } else {
            Ok(Response::Observation(
                Observation::from_bytes(&bytes).unwrap(),
            ))
        }
    }
    pub fn accord(&self, token: PendId, target: TaskId, limits: Limits) -> PendId {
        let channel = &CHANNELS[self.1];
        let bytes = AccordRequest {
            src: token,
            target: AccordTarget::Task(target),
            limits,
            mark: Mark::NONE,
        }
        .to_bytes();
        for (out, byte) in channel.bytes.iter().zip(bytes) {
            out.store(byte, Ordering::Relaxed);
        }
        channel.state.store(State::Accord as u8, Ordering::Release);
        let end = room::clock() + 3_000_000_000;
        while channel.state.load(Ordering::Acquire) != State::Token as u8 {
            assert!(room::clock() < end);
            room::yield_();
        }
        let bytes = core::array::from_fn(|i| channel.bytes[i].load(Ordering::Relaxed));
        let token = PendId::mint(usize::from_le_bytes(bytes));
        channel.state.store(State::Idle as u8, Ordering::Release);
        token
    }
    pub fn stop(self) {
        let channel = &CHANNELS[self.1];
        channel.state.store(State::Stop as u8, Ordering::Release);
        assert_eq!(join(self.0).result, Some(81));
        channel.leased.store(false, Ordering::Release);
    }
}
extern "C" fn entry0() -> ! {
    entry(&CHANNELS[0])
}
extern "C" fn entry1() -> ! {
    entry(&CHANNELS[1])
}
fn entry(channel: &Channel) -> ! {
    loop {
        match channel.state.load(Ordering::Acquire) {
            x if x == State::Request as u8 => {
                let bytes = core::array::from_fn::<_, { Request::WIDTH }, _>(|i| {
                    channel.bytes[i].load(Ordering::Relaxed)
                });
                let action = Request::from_bytes(&bytes).unwrap().action();
                let (bytes, state) = match try_call(action) {
                    Ok(Response::Reply(r)) => (r.to_bytes(), State::Reply),
                    Ok(Response::Observation(o)) => (o.to_bytes(), State::Observation),
                    Err(kernel_client::memory::Error::Delivery(f)) => {
                        let mut bytes = [0; 128];
                        bytes[..8].copy_from_slice(&f.code().to_le_bytes());
                        (bytes, State::Error)
                    }
                    Err(e) => panic!("{e:?}"),
                };
                for (out, byte) in channel.bytes.iter().zip(bytes) {
                    out.store(byte, Ordering::Relaxed);
                }
                channel.state.store(state as u8, Ordering::Release);
            }
            x if x == State::Accord as u8 => {
                let bytes = core::array::from_fn::<_, { AccordRequest::WIDTH }, _>(|i| {
                    channel.bytes[i].load(Ordering::Relaxed)
                });
                let token = {
 let deadline = kernel_client::call::room::clock() + 5_000_000_000;
 loop {
 match pend::accord(VirtAddr::new(bytes.as_ptr() as usize)) {
 Ok(token) => break token,
 Err(error) => assert_eq!(error.into_source(), kernel_api::abi::call::pend::PendFail::Busy),
 }
 assert!(kernel_client::call::room::clock() < deadline);
 kernel_client::call::room::yield_();
 }
 };
                for (out, byte) in channel.bytes.iter().zip(token.get().to_le_bytes()) {
                    out.store(byte, Ordering::Relaxed);
                }
                channel.state.store(State::Token as u8, Ordering::Release);
            }
            x if x == State::Stop as u8 => unit::exit(81, 0, VirtAddr::new(0), 0, ExitScope::Task),
            _ => room::yield_(),
        }
    }
}
