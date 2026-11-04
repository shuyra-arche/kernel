use env as old_env;
use kernel_api::env as api_env;

fn from_api_permission(value: api_env::Permission) -> old_env::Permission {
    value
}

fn from_old_permission(value: old_env::Permission) -> api_env::Permission {
    value
}

fn from_api_memory_call(value: api_env::MemoryCall) -> old_env::MemoryCall {
    value
}

fn from_old_memory_call(value: old_env::MemoryCall) -> api_env::MemoryCall {
    value
}

fn shared_wire<T: old_env::Wire + api_env::Wire>() {}

fn shared_fail_code<T: old_env::FailCode + api_env::FailCode>() {}

#[test]
fn candidate_env_paths_are_the_existing_env_types_in_both_directions() {
    let old = old_env::Permission::FETCH | old_env::Permission::STORE | old_env::Permission::VEST;
    let api = from_old_permission(old);
    let round_trip = from_api_permission(api);
    assert_eq!(round_trip, old);
    assert_eq!(round_trip.bits(), 0b0111);

    let old_access = old_env::Access::FETCH_STORE;
    let api_access: api_env::Access = old_access;
    let old_again: old_env::Access = api_access;
    assert_eq!(old_again, old_env::Access::FETCH_STORE);
    assert_eq!(old_again.bits().bits(), 0b0011);

    let api_policy: api_env::Policy = old_env::Policy::VEST;
    let old_policy: old_env::Policy = api_policy;
    assert_eq!(old_policy.bits().bits(), 0b0100);

    // These signatures are only well-typed when the candidate path names the
    // same nominal `MemoryCall` type as the existing `env` crate.
    let _: fn(api_env::MemoryCall) -> old_env::MemoryCall = from_api_memory_call;
    let _: fn(old_env::MemoryCall) -> api_env::MemoryCall = from_old_memory_call;

    shared_wire::<usize>();
    shared_wire::<old_env::PieToken>();
    shared_fail_code::<old_env::MemoryFail>();
}
