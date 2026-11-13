use kernel_api::{
    abi::memory::{
        self, Action, Delivery, Domain, Failure, Observation, Opcode, Outcome, Query, Reply,
        RequestId, Snapshot, Value,
    },
    address::VirtAddr,
    model::{
        cell::{CellLevel, MappingId},
        identity::PendId,
        nest::{MappingPhase, MappingStatus, MappingVersion, NestId, NestVersion},
    },
};
use kernel_client::memory::{Error, Response, invoke_with};
fn call() -> memory::nest::Call {
    memory::nest::Call::new(
        RequestId::new(7),
        Action::CreateNest {
            license: PendId::mint(3),
        },
    )
    .unwrap()
}
fn committed(id: u64) -> Reply {
    Reply::new(
        RequestId::new(id).unwrap(),
        Opcode::Create(Domain::Nest),
        Outcome::Committed(Value::CreatedNest {
            token: PendId::mint(17),
            nest: NestId::new(19).unwrap(),
            version: NestVersion::INITIAL,
        }),
    )
    .unwrap()
}
#[test]
fn one_canonical_snapshot_and_one_correlated_result() {
    let request = call();
    let result = committed(7);
    let mut calls = 0;
    assert_eq!(
        invoke_with(request, |number, input, output| {
            calls += 1;
            assert_eq!(number, Opcode::Create(Domain::Nest).call_number());
            assert_eq!(input, &request.request().to_bytes());
            assert_eq!(output, &[0; 128]);
            *output = result.to_bytes();
            Delivery::Written.registers()
        }),
        Ok(Response::Reply(result))
    );
    assert_eq!(calls, 1);
}
#[test]
fn lost_delivery_and_malformed_success_never_trigger_retry_or_fake_rejection() {
    let mut calls = 0;
    assert_eq!(
        invoke_with(call(), |_, _, output| {
            calls += 1;
            *output = committed(7).to_bytes();
            Delivery::Failed(Failure::ReplyFault).registers()
        }),
        Err(Error::Delivery(Failure::ReplyFault))
    );
    assert_eq!(calls, 1);
    for registers in [[0, 127, 0], [0, 128, 1], [0, 0, 0], [u64::MAX, 8, 0]] {
        assert_eq!(
            invoke_with(call(), |_, _, out| {
                *out = committed(7).to_bytes();
                registers
            }),
            Err(Error::Protocol)
        );
    }
    assert_eq!(
        invoke_with(call(), |_, _, out| {
            *out = committed(8).to_bytes();
            Delivery::Written.registers()
        }),
        Err(Error::Protocol)
    );
    assert_eq!(
        invoke_with(call(), |_, _, _| Delivery::Written.registers()),
        Err(Error::Protocol)
    );
}
#[test]
fn accepted_failure_and_recovered_original_opcode_stay_distinct_from_delivery() {
    let failure = Reply::new(
        RequestId::new(7).unwrap(),
        Opcode::Create(Domain::Nest),
        Outcome::Rejected(Failure::Unsupported),
    )
    .unwrap();
    assert_eq!(
        invoke_with(call(), |_, _, out| {
            *out = failure.to_bytes();
            Delivery::Written.registers()
        }),
        Ok(Response::Reply(failure))
    );
    let inspect = memory::nest::Call::new(
        None,
        Action::InspectResult {
            domain: Domain::Nest,
            request: RequestId::new(7).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(
        invoke_with(inspect, |_, _, out| {
            *out = committed(7).to_bytes();
            Delivery::Written.registers()
        }),
        Ok(Response::Reply(committed(7)))
    );
    let expired = Reply::new(
        RequestId::new(7).unwrap(),
        Opcode::Inspect(Domain::Nest),
        Outcome::Expired,
    )
    .unwrap();
    assert_eq!(
        invoke_with(inspect, |_, _, out| {
            *out = expired.to_bytes();
            Delivery::Written.registers()
        }),
        Ok(Response::Reply(expired))
    );
}
#[test]
fn observations_require_the_requested_mapping_and_cannot_replace_mutation_results() {
    let nest = NestId::new(19).unwrap();
    let mapping = MappingId::from_bytes(&23u64.to_le_bytes()).unwrap();
    let request = memory::nest::Call::new(
        None,
        Action::Inspect(Query::Mapping {
            token: PendId::mint(17),
            nest,
            mapping,
        }),
    )
    .unwrap();
    let status = MappingStatus {
        nest,
        mapping,
        version: MappingVersion::INITIAL,
        offset: 0,
        count: 1,
        address: VirtAddr::new(4096),
        level: CellLevel::BASE,
        access: 2,
        phase: MappingPhase::Open,
    };
    let observation = Observation::new(Snapshot::Mapping(status)).unwrap();
    assert_eq!(
        invoke_with(request, |_, _, out| {
            *out = observation.to_bytes();
            Delivery::Written.registers()
        }),
        Ok(Response::Observation(observation))
    );
    assert_eq!(
        invoke_with(call(), |_, _, out| {
            *out = observation.to_bytes();
            Delivery::Written.registers()
        }),
        Err(Error::Protocol)
    );
    let wrong = Observation::new(Snapshot::Mapping(MappingStatus {
        nest: NestId::new(20).unwrap(),
        ..status
    }))
    .unwrap();
    assert_eq!(
        invoke_with(request, |_, _, out| {
            *out = wrong.to_bytes();
            Delivery::Written.registers()
        }),
        Err(Error::Protocol)
    );
}
