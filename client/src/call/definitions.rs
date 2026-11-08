// Client-owned signatures. ABI values and codecs remain in kernel_api.
typed_calls! {
    debug [
        abi::call::debug::DebugFail,
        address::VirtAddr,
    ] {
        fallible fn put(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
            op::Put { buf, len };
        fallible fn get(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
            op::Get { buf, len };
    }
    clone_view [
        abi::call::clone_view::CloneFail,
        model::clone_view::CloneLimits,
        model::identity::Mark,
        model::identity::PendId,
        model::clone_view::PullOutcome,
        model::identity::TaskId,
        address::VirtAddr,
    ] {
        fallible fn create(mark: Mark, limits: CloneLimits) -> Result<PendId, erra::Error<CloneFail>> =
            op::Create { mark, limits };
        fallible fn delete(token: PendId) -> Result<(), erra::Error<CloneFail>> =
            op::Delete { token };
        fallible fn push(token: PendId, msg: VirtAddr, len: usize) -> Result<(), erra::Error<CloneFail>> =
            op::Push { token, msg, len };
        fallible fn pull(token: PendId, max_len: usize) -> Result<PullOutcome, erra::Error<CloneFail>> =
            op::Pull { token, max_len };
        fallible fn peek(token: PendId) -> Result<(usize, TaskId, usize), erra::Error<CloneFail>> =
            op::Peek { token };
    }
    cell [abi::call::cell::CellFail, model::cell::CreatedCell, address::VirtAddr] {
        fallible fn create(request: VirtAddr) -> Result<CreatedCell, erra::Error<CellFail>> = op::Create { request };
        fallible fn delete(object: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Delete { object };
        fallible fn push(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Push { request };
        fallible fn pull(request: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Pull { request };
        fallible fn publish(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Publish { position };
        fallible fn retract(position: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Retract { position };
        fallible fn protect(position: VirtAddr, access: u64) -> Result<(), erra::Error<CellFail>> = op::Protect { position, access };
        fallible fn inspect(position: VirtAddr, out: VirtAddr) -> Result<(), erra::Error<CellFail>> = op::Inspect { position, out };
    }
    pend [
        model::modifiable::Modifiable,
        abi::call::pend::PendFail,
        model::identity::PendId,
        model::identity::TaskId,
        address::VirtAddr,
    ] {
        fallible fn accord(request: VirtAddr) -> Result<PendId, erra::Error<PendFail>> =
            op::Accord { request };
        fallible fn narrow(token: PendId, subset: Modifiable, range: kernel_api::model::pend::PendRange) -> Result<(), erra::Error<PendFail>> =
            op::Narrow { token, subset, range };
        fallible fn revoke(dst: TaskId, token: PendId) -> Result<(), erra::Error<PendFail>> =
            op::Revoke { dst, token };
        fallible fn return_(token: PendId) -> Result<(), erra::Error<PendFail>> =
            op::Return { token };
        fallible fn inspect(token: PendId, buf: VirtAddr) -> Result<(), erra::Error<PendFail>> =
            op::Inspect { token, buf };
        fallible fn collect(after: PendId, buf: VirtAddr, capacity: usize) -> Result<usize, erra::Error<PendFail>> =
            op::Collect { after, buf, capacity };
    }
    share_view [
        model::modifiable::Modifiable,
        model::cell::SlotId,
        model::identity::PendId,
        abi::call::share_view::ShareFail,
        address::VirtAddr,
    ] {
        fallible fn create(reference: SlotId, modifiable: Modifiable) -> Result<PendId, erra::Error<ShareFail>> =
            op::Create { reference, modifiable };
        fallible fn delete(token: PendId) -> Result<(), erra::Error<ShareFail>> =
            op::Delete { token };
        fallible fn open(token: PendId) -> Result<(VirtAddr, usize), erra::Error<ShareFail>> =
            op::Open { token };
        fallible fn shut(token: PendId) -> Result<(), erra::Error<ShareFail>> =
            op::Shut { token };
    }
    room [address::VirtAddr, abi::call::room::RoomFail, model::wait::Wait] {
        infallible fn yield_() -> () = op::Yield;
        infallible fn clock() -> u64 = op::Clock;
        fallible fn sleep_until(at: u64) -> Result<(), erra::Error<RoomFail>> = op::SleepUntil { at };
        fallible fn wait_value(address: VirtAddr, expected: u32, wait: Wait) -> Result<bool, erra::Error<RoomFail>> = op::WaitValue { address, expected, wait };
        fallible fn wake_value(address: VirtAddr, all: bool) -> Result<usize, erra::Error<RoomFail>> = op::WakeValue { address, all };
    }
    sign [
        model::identity::TaskId, model::sign::{SignId, SignRights}, abi::reply::SignReply,
        model::sign::SignSource, abi::call::sign::SignFail, model::wait::Wait,
    ] {
        fallible fn create() -> Result<SignId, erra::Error<SignFail>> = op::Create;
        fallible fn delete(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Delete { token };
        fallible fn listen(token: SignId, wait: Wait) -> Result<SignReply, erra::Error<SignFail>> = op::Listen { token, wait };
        fallible fn trigger(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Trigger { token };
        fallible fn consume(token: SignId, sequence: u64) -> Result<(), erra::Error<SignFail>> = op::Consume { token, sequence };
        fallible fn attach(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Attach { token, source };
        fallible fn detach(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Detach { token, source };
        fallible fn accord(token: SignId, target: TaskId, rights: SignRights) -> Result<SignId, erra::Error<SignFail>> = op::Accord { token, target, rights };
        fallible fn return_(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Return { token };
        fallible fn revoke(token: SignId, target: TaskId, branch: SignId) -> Result<(), erra::Error<SignFail>> = op::Revoke { token, target, branch };
    }
    unit [address::VirtAddr, model::identity::{TaskId, TeamId}, model::unit::{ControlId, UnitQuery, UnitTarget}, abi::call::unit::UnitFail] {
        fallible fn create(request: VirtAddr) -> Result<UnitTarget, erra::Error<UnitFail>> = op::Create { request };
        fallible fn delete(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Delete { target };
        fallible fn resume(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Resume { target };
        fallible fn freeze(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Freeze { target };
        fallible fn stop(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> = op::Stop { target };
        terminal fn exit(value: u64, reason: u64, note: VirtAddr, len: usize) -> ! = op::Exit { value, reason, note, len };
        fallible fn finish(team: TeamId) -> Result<(), erra::Error<UnitFail>> = op::Finish { team };
        fallible fn inspect(query: UnitQuery, out: VirtAddr) -> Result<bool, erra::Error<UnitFail>> = op::Inspect { query, out };
        fallible fn accord(authority: ControlId, target: TaskId, delegable: bool) -> Result<ControlId, erra::Error<UnitFail>> = op::Accord { authority, target, delegable };
        fallible fn revoke(authority: ControlId, target: TaskId, branch: ControlId) -> Result<(), erra::Error<UnitFail>> = op::Revoke { authority, target, branch };
        fallible fn return_(authority: ControlId) -> Result<(), erra::Error<UnitFail>> = op::Return { authority };
    }
}
