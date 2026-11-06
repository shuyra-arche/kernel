#[test]
fn public_operation_types_enforce_the_register_contract() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/abi/fixtures/pass/*.rs");
    cases.compile_fail("tests/abi/fixtures/fail/*.rs");
}
