// Client-owned signatures. ABI values and codecs remain in kernel_api.
typed_calls! {
    chrono [] {
        infallible fn ticks() -> usize =
            op::Ticks;
        infallible fn clock() -> u64 =
            op::Clock;
    }
    control [
        abi::call::control::ControlFail,
    ] {
        fallible fn backtrace(buf: usize, frames: usize) -> Result<usize, erra::Error<ControlFail>> =
            op::Backtrace { buf, frames };
    }
    debug [
        abi::call::debug::DebugFail,
        model::identity::VirtAddr,
    ] {
        fallible fn put(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
            op::Put { buf, len };
        fallible fn get(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
            op::Get { buf, len };
        infallible fn set_trace(on: usize) -> () =
            op::SetTrace { on };
    }
    clone_view [
        model::clone_view::CloneCondition,
        abi::call::clone_view::CloneFail,
        model::clone_view::CloneLimits,
        model::identity::Mark,
        model::clone_view::Oversize,
        model::identity::PendId,
        model::clone_view::PullOutcome,
        model::identity::TaskId,
        model::identity::VirtAddr,
        model::wait::Wait,
    ] {
        fallible fn unseal(mark: Mark, limits: CloneLimits) -> Result<PendId, erra::Error<CloneFail>> =
            op::Unseal { mark, limits };
        fallible fn seal(token: PendId) -> Result<(), erra::Error<CloneFail>> =
            op::Seal { token };
        fallible fn push(token: PendId, msg: VirtAddr, len: usize) -> Result<(), erra::Error<CloneFail>> =
            op::Push { token, msg, len };
        fallible fn pull(token: PendId, buf: VirtAddr, max: usize, oversize: Oversize) -> Result<PullOutcome, erra::Error<CloneFail>> =
            op::Pull { token, buf, max, oversize };
        fallible fn peek(token: PendId) -> Result<(usize, TaskId, usize), erra::Error<CloneFail>> =
            op::Peek { token };
        fallible fn wait(token: PendId, condition: CloneCondition, millis: Wait) -> Result<bool, erra::Error<CloneFail>> =
            op::Wait { token, condition, millis };
    }
    memory [
        model::memory::AcquireSource,
        model::memory::FlatReference,
        abi::call::memory::MemoryFail,

        model::identity::TeamId,
        model::memory::MappingControl,
        model::memory::MappingId,
        model::memory::MappingSelection,
        model::identity::VirtAddr,
    ] {
        fallible fn acquire(source: AcquireSource, retention: kernel_api::model::unit::ControlId) -> Result<FlatReference, erra::Error<MemoryFail>> =
            op::Acquire { source, retention };
        fallible fn release(reference: FlatReference) -> Result<(), erra::Error<MemoryFail>> =
            op::Release { reference };
        fallible fn attach(range: VirtAddr, team: TeamId, placement: VirtAddr, access: u64, control: MappingControl) -> Result<(MappingId, VirtAddr), erra::Error<MemoryFail>> =
            op::Attach { range, team, placement, access, control };
        fallible fn detach(team: TeamId, mapping: MappingId, selection: MappingSelection) -> Result<(), erra::Error<MemoryFail>> =
            op::Detach { team, mapping, selection };
        fallible fn set_access(team: TeamId, mapping: MappingId, selection: MappingSelection, access: u64) -> Result<(), erra::Error<MemoryFail>> =
            op::SetAccess { team, mapping, selection, access };
        fallible fn traverse(retention: kernel_api::model::unit::ControlId, range: VirtAddr, out: VirtAddr) -> Result<(), erra::Error<MemoryFail>> =
            op::Traverse { retention, range, out };
        fallible fn transfer(from: FlatReference, from_page: usize, to: FlatReference, to_page: usize) -> Result<(), erra::Error<MemoryFail>> =
            op::Transfer { from, from_page, to, to_page };
    }
    pend [
        model::pend::PendRange,
        model::identity::Mark,
        model::modifiable::Modifiable,
        abi::call::pend::PendFail,
        model::identity::PendId,
        model::pend::ReleaseMode,
        model::identity::TaskId,
        model::identity::VirtAddr,
    ] {
        fallible fn accord(src: PendId, dst: TaskId, subset: Modifiable, mark: Mark) -> Result<PendId, erra::Error<PendFail>> =
            op::Accord { src, dst, subset, mark };
        fallible fn narrow(token: PendId, subset: Modifiable) -> Result<(), erra::Error<PendFail>> =
            op::Narrow { token, subset };
        fallible fn revoke(dst: TaskId, token: PendId) -> Result<(), erra::Error<PendFail>> =
            op::Revoke { dst, token };
        fallible fn release(token: PendId, mode: ReleaseMode) -> Result<(), erra::Error<PendFail>> =
            op::Release { token, mode };
        fallible fn inspect(token: PendId, buf: VirtAddr) -> Result<(), erra::Error<PendFail>> =
            op::Inspect { token, buf };
        fallible fn collect(after: PendId, buf: VirtAddr, capacity: usize) -> Result<usize, erra::Error<PendFail>> =
            op::Collect { after, buf, capacity };
        fallible fn same(a: PendId, b: PendId) -> Result<bool, erra::Error<PendFail>> =
            op::Same { a, b };
        fallible fn derive(src: PendId, subset: Modifiable, mark: Mark, range: PendRange) -> Result<PendId, erra::Error<PendFail>> =
            op::Derive { src, subset, mark, range };
    }
    share_view [
        model::modifiable::Modifiable,
        model::memory::FlatReference,
        model::identity::PendId,
        abi::call::share_view::ShareFail,
        model::identity::VirtAddr,
    ] {
        fallible fn unseal(reference: FlatReference, modifiable: Modifiable) -> Result<PendId, erra::Error<ShareFail>> =
            op::Unseal { reference, modifiable };
        fallible fn seal(token: PendId) -> Result<(), erra::Error<ShareFail>> =
            op::Seal { token };
        fallible fn open(token: PendId) -> Result<(VirtAddr, usize), erra::Error<ShareFail>> =
            op::Open { token };
        fallible fn shut(token: PendId) -> Result<(), erra::Error<ShareFail>> =
            op::Shut { token };
    }
    room [
        model::exit::Reason,
        model::identity::VirtAddr,
        abi::call::room::RoomFail,
        model::identity::TaskId,
        model::wait::Wait,
    ] {
        terminal fn reap(reason: Reason, note: VirtAddr, len: usize) -> ! =
            op::Reap { reason, note, len };
        infallible fn starve() -> () =
            op::Starve;
        fallible fn park(millis: usize) -> Result<(), erra::Error<RoomFail>> =
            op::Park { millis };
        fallible fn wait(key: usize, millis: Wait) -> Result<(), erra::Error<RoomFail>> =
            op::Wait { key, millis };
        infallible fn wake(key: usize) -> bool =
            op::Wake { key };
        fallible fn wait_value(address: VirtAddr, expected: u32, millis: Wait) -> Result<bool, erra::Error<RoomFail>> =
            op::WaitValue { address, expected, millis };
        fallible fn wake_value(address: VirtAddr, all: bool) -> Result<usize, erra::Error<RoomFail>> =
            op::WakeValue { address, all };
        fallible fn doom(task: TaskId) -> Result<(), erra::Error<RoomFail>> =
            op::Doom { task };
        fallible fn park_until(at: u64) -> Result<(), erra::Error<RoomFail>> =
            op::ParkUntil { at };
    }
    sign [
        model::identity::TaskId, model::sign::{SignId, SignRights}, abi::reply::SignReply,
        model::sign::SignSource, abi::call::sign::SignFail, model::wait::Wait,
    ] {
        fallible fn next(after: usize) -> Result<(usize, usize), erra::Error<SignFail>> = op::Next { after };
        fallible fn create() -> Result<SignId, erra::Error<SignFail>> = op::Create;
        fallible fn close(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Close { token };
        fallible fn listen(token: SignId, millis: Wait) -> Result<SignReply, erra::Error<SignFail>> = op::Listen { token, millis };
        fallible fn trigger(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Trigger { token };
        fallible fn consume(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Consume { token };
        fallible fn route(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Route { token, source };
        fallible fn unroute(token: SignId, source: SignSource) -> Result<(), erra::Error<SignFail>> = op::Unroute { token, source };
        fallible fn delegate(token: SignId, target: TaskId, rights: SignRights) -> Result<SignId, erra::Error<SignFail>> = op::Delegate { token, target, rights };
        fallible fn release(token: SignId) -> Result<(), erra::Error<SignFail>> = op::Release { token };
    }
    device [model::identity::TeamId, model::memory::FlatReference, abi::call::device::DeviceFail] {
        fallible fn fault(device: usize) -> Result<(usize, usize), erra::Error<DeviceFail>> = op::Fault { device };
        fallible fn observe(device: usize) -> Result<usize, erra::Error<DeviceFail>> = op::Observe { device };
        fallible fn publish(device: usize, reference: FlatReference, iova: usize, modifiable: bool) -> Result<(), erra::Error<DeviceFail>> = op::Publish { device, reference, iova, modifiable };
        fallible fn status(device: usize) -> Result<usize, erra::Error<DeviceFail>> = op::Status { device };
        fallible fn retire(device: usize) -> Result<(), erra::Error<DeviceFail>> = op::Retire { device };
        fallible fn scan(after: usize) -> Result<(usize, usize), erra::Error<DeviceFail>> = op::Scan { after };
        fallible fn claim(device: usize, team: TeamId) -> Result<(), erra::Error<DeviceFail>> = op::Claim { device, team };
        fallible fn inspect(device: usize, offset: usize) -> Result<usize, erra::Error<DeviceFail>> = op::Inspect { device, offset };
        fallible fn read(device: usize, offset: usize, width: usize) -> Result<(usize, usize), erra::Error<DeviceFail>> = op::Read { device, offset, width };
        fallible fn write(device: usize, offset: usize, width: usize, value: u64) -> Result<(), erra::Error<DeviceFail>> = op::Write { device, offset, width, value };
    }
    unit [
        model::unit::JoinReply,

        model::unit::ProgramKind,
        model::identity::TaskId,
        model::identity::TeamId,
        abi::call::unit::UnitFail,
        model::unit::UnitTarget,
        model::identity::VirtAddr,
        model::wait::Wait,
    ] {
        fallible fn build(kind: ProgramKind, sire: TaskId) -> Result<TeamId, erra::Error<UnitFail>> =
            op::Build { kind, sire };
        fallible fn spawn(team: TeamId, entry: usize, args: VirtAddr, count: usize, stack: usize) -> Result<TaskId, erra::Error<UnitFail>> =
            op::Spawn { team, entry, args, count, stack };
        fallible fn embark(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> =
            op::Embark { target };
        fallible fn debark(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> =
            op::Debark { target };
        fallible fn slay(target: UnitTarget) -> Result<(), erra::Error<UnitFail>> =
            op::Slay { target };
        fallible fn join(target: UnitTarget, millis: Wait, receive: bool) -> Result<JoinReply, erra::Error<UnitFail>> =
            op::Join { target, millis, receive };
        fallible fn oust(team: TeamId) -> Result<(), erra::Error<UnitFail>> =
            op::Oust { team };
        fallible fn scan(after: TeamId, buf: VirtAddr, capacity: usize) -> Result<usize, erra::Error<UnitFail>> =
            op::Scan { after, buf, capacity };
        infallible fn self_id() -> TaskId =
            op::SelfId;
        infallible fn sire() -> TaskId =
            op::Sire;
        fallible fn fall(millis: Wait) -> Result<bool, erra::Error<UnitFail>> =
            op::Fall { millis };
        fallible fn finish(team: TeamId) -> Result<(), erra::Error<UnitFail>> = op::Finish { team };
        terminal fn return_(value: usize) -> ! = op::Return { value };
        fallible fn read_result(task: TaskId) -> Result<(usize, usize), erra::Error<UnitFail>> = op::ReadResult { task };
        fallible fn delegate_control(authority: kernel_api::model::unit::ControlId, target: TaskId, delegable: bool) -> Result<kernel_api::model::unit::ControlId, erra::Error<UnitFail>> = op::DelegateControl { authority, target, delegable };
        fallible fn release_control(authority: kernel_api::model::unit::ControlId) -> Result<(), erra::Error<UnitFail>> = op::ReleaseControl { authority };
        fallible fn guard(team: TeamId, authority: kernel_api::model::unit::ControlId) -> Result<(), erra::Error<UnitFail>> =
            op::Guard { team, authority };
        fallible fn unguard(team: TeamId, authority: kernel_api::model::unit::ControlId) -> Result<(), erra::Error<UnitFail>> =
            op::Unguard { team, authority };
        infallible fn creator() -> TaskId =
            op::Creator;
    }
}
