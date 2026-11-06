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
    hole [
        model::hole::HoleCondition,
        abi::call::hole::HoleFail,
        model::hole::HoleLimits,
        model::identity::Mark,
        model::hole::Oversize,
        model::identity::PieToken,
        model::hole::PullOutcome,
        model::identity::TaskId,
        model::identity::VirtAddr,
        model::wait::Wait,
    ] {
        fallible fn unseal(mark: Mark, limits: HoleLimits) -> Result<PieToken, erra::Error<HoleFail>> =
            op::Unseal { mark, limits };
        fallible fn seal(token: PieToken) -> Result<(), erra::Error<HoleFail>> =
            op::Seal { token };
        fallible fn push(token: PieToken, msg: VirtAddr, len: usize) -> Result<(), erra::Error<HoleFail>> =
            op::Push { token, msg, len };
        fallible fn pull(token: PieToken, buf: VirtAddr, max: usize, oversize: Oversize) -> Result<PullOutcome, erra::Error<HoleFail>> =
            op::Pull { token, buf, max, oversize };
        fallible fn peek(token: PieToken) -> Result<(usize, TaskId, usize), erra::Error<HoleFail>> =
            op::Peek { token };
        fallible fn wait(token: PieToken, condition: HoleCondition, millis: Wait) -> Result<bool, erra::Error<HoleFail>> =
            op::Wait { token, condition, millis };
        fallible fn ring(token: PieToken) -> Result<(), erra::Error<HoleFail>> =
            op::Ring { token };
        fallible fn hush(token: PieToken) -> Result<(), erra::Error<HoleFail>> =
            op::Hush { token };
    }
    memory [
        model::memory::AcquireSource,
        model::memory::FlatReference,
        abi::call::memory::MemoryFail,
        model::identity::PieToken,
        model::identity::TeamId,
        model::memory::ViewControl,
        model::memory::ViewId,
        model::memory::ViewSelection,
        model::identity::VirtAddr,
    ] {
        fallible fn acquire(source: AcquireSource, retention: PieToken) -> Result<FlatReference, erra::Error<MemoryFail>> =
            op::Acquire { source, retention };
        fallible fn release(reference: FlatReference) -> Result<(), erra::Error<MemoryFail>> =
            op::Release { reference };
        fallible fn attach(range: VirtAddr, team: TeamId, placement: VirtAddr, access: u64, control: ViewControl) -> Result<(ViewId, VirtAddr), erra::Error<MemoryFail>> =
            op::Attach { range, team, placement, access, control };
        fallible fn detach(team: TeamId, view: ViewId, selection: ViewSelection) -> Result<(), erra::Error<MemoryFail>> =
            op::Detach { team, view, selection };
        fallible fn set_access(team: TeamId, view: ViewId, selection: ViewSelection, access: u64) -> Result<(), erra::Error<MemoryFail>> =
            op::SetAccess { team, view, selection, access };
        fallible fn traverse(retention: PieToken, range: VirtAddr, out: VirtAddr) -> Result<(), erra::Error<MemoryFail>> =
            op::Traverse { retention, range, out };
        fallible fn transfer(from: FlatReference, from_page: usize, to: FlatReference, to_page: usize) -> Result<(), erra::Error<MemoryFail>> =
            op::Transfer { from, from_page, to, to_page };
    }
    nole [
        abi::call::nole::NoleFail,
        model::identity::PieToken,
        model::wait::Wait,
    ] {
        fallible fn unseal() -> Result<PieToken, erra::Error<NoleFail>> =
            op::Unseal;
        fallible fn seal(token: PieToken) -> Result<(), erra::Error<NoleFail>> =
            op::Seal { token };
        fallible fn wait(token: PieToken, millis: Wait) -> Result<bool, erra::Error<NoleFail>> =
            op::Wait { token, millis };
        fallible fn ring(token: PieToken) -> Result<(), erra::Error<NoleFail>> =
            op::Ring { token };
        fallible fn hush(token: PieToken) -> Result<(), erra::Error<NoleFail>> =
            op::Hush { token };
    }
    pie [
        model::pie::GrantRange,
        model::identity::Mark,
        model::permission::Permission,
        abi::call::pie::PieFail,
        model::identity::PieToken,
        model::pie::ReleaseMode,
        model::identity::TaskId,
        model::identity::VirtAddr,
    ] {
        fallible fn accord(src: PieToken, dst: TaskId, subset: Permission, mark: Mark) -> Result<PieToken, erra::Error<PieFail>> =
            op::Accord { src, dst, subset, mark };
        fallible fn narrow(token: PieToken, subset: Permission) -> Result<(), erra::Error<PieFail>> =
            op::Narrow { token, subset };
        fallible fn revoke(dst: TaskId, token: PieToken) -> Result<(), erra::Error<PieFail>> =
            op::Revoke { dst, token };
        fallible fn release(token: PieToken, mode: ReleaseMode) -> Result<(), erra::Error<PieFail>> =
            op::Release { token, mode };
        fallible fn inspect(token: PieToken, buf: VirtAddr) -> Result<(), erra::Error<PieFail>> =
            op::Inspect { token, buf };
        fallible fn collect(after: PieToken, buf: VirtAddr, capacity: usize) -> Result<usize, erra::Error<PieFail>> =
            op::Collect { after, buf, capacity };
        fallible fn same(a: PieToken, b: PieToken) -> Result<bool, erra::Error<PieFail>> =
            op::Same { a, b };
        fallible fn derive(src: PieToken, subset: Permission, mark: Mark, range: GrantRange) -> Result<PieToken, erra::Error<PieFail>> =
            op::Derive { src, subset, mark, range };
    }
    pole [
        model::pole::Bit,
        model::memory::FlatReference,
        model::identity::PieToken,
        model::pole::PoleCondition,
        abi::call::pole::PoleFail,
        model::identity::VirtAddr,
        model::wait::Wait,
    ] {
        fallible fn unseal(reference: FlatReference, shared: bool) -> Result<PieToken, erra::Error<PoleFail>> =
            op::Unseal { reference, shared };
        fallible fn seal(token: PieToken) -> Result<(), erra::Error<PoleFail>> =
            op::Seal { token };
        fallible fn open(token: PieToken) -> Result<(VirtAddr, usize), erra::Error<PoleFail>> =
            op::Open { token };
        fallible fn shut(token: PieToken) -> Result<(), erra::Error<PoleFail>> =
            op::Shut { token };
        fallible fn wait(token: PieToken, condition: PoleCondition, millis: Wait) -> Result<bool, erra::Error<PoleFail>> =
            op::Wait { token, condition, millis };
        fallible fn ring(token: PieToken, bit: Bit) -> Result<(), erra::Error<PoleFail>> =
            op::Ring { token, bit };
        fallible fn hush(token: PieToken, bit: Bit) -> Result<(), erra::Error<PoleFail>> =
            op::Hush { token, bit };
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
        fallible fn doom(task: TaskId) -> Result<(), erra::Error<RoomFail>> =
            op::Doom { task };
        fallible fn park_until(at: u64) -> Result<(), erra::Error<RoomFail>> =
            op::ParkUntil { at };
    }
    tole [
        model::identity::PieToken,
        abi::reply::ToleAwaitReply,
        model::tole::ToleEventSource,
        abi::call::tole::ToleFail,
        model::wait::Wait,
    ] {
        fallible fn unseal(shared: bool) -> Result<PieToken, erra::Error<ToleFail>> =
            op::Unseal { shared };
        fallible fn seal(token: PieToken) -> Result<(), erra::Error<ToleFail>> =
            op::Seal { token };
        fallible fn attach(tole: PieToken, source: ToleEventSource) -> Result<(), erra::Error<ToleFail>> =
            op::Attach { tole, source };
        fallible fn detach(tole: PieToken, source: ToleEventSource) -> Result<(), erra::Error<ToleFail>> =
            op::Detach { tole, source };
        fallible fn await_(tole: PieToken, millis: Wait) -> Result<ToleAwaitReply, erra::Error<ToleFail>> =
            op::Await { tole, millis };
    }
    unit [
        model::unit::JoinReply,
        model::identity::PieToken,
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
        fallible fn guard(team: TeamId, authority: PieToken) -> Result<(), erra::Error<UnitFail>> =
            op::Guard { team, authority };
        fallible fn unguard(team: TeamId, authority: PieToken) -> Result<(), erra::Error<UnitFail>> =
            op::Unguard { team, authority };
        infallible fn creator() -> TaskId =
            op::Creator;
    }
}
