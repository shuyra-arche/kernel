#![allow(dead_code)]

fn same_task(a: kernel_api::env::TaskId) -> kernel_std::env::TaskId {
    a
}

fn same_team(a: kernel_api::env::TeamId) -> kernel_std::env::TeamId {
    a
}

fn same_view(a: kernel_api::env::ViewId) -> kernel_std::env::ViewId {
    a
}

fn same_pie(a: kernel_api::env::PieToken) -> kernel_std::env::PieToken {
    a
}

fn same_reference(a: kernel_api::env::FlatReference) -> kernel_std::env::FlatReference {
    a
}

fn same_reason(a: kernel_api::env::Reason) -> kernel_std::env::Reason {
    a
}

fn same_permission(a: kernel_api::env::Permission) -> kernel_std::env::Permission {
    a
}

#[test]
fn public_std_api_uses_canonical_abi_types() {
    let task = kernel_std::env::TaskId::new(3);
    let team = kernel_std::env::TeamId::new(4);
    let view = kernel_std::env::ViewId::from_bytes(&5usize.to_le_bytes()).unwrap();
    let pie = kernel_std::env::PieToken::mint(6);
    let reference = kernel_std::env::FlatReference::from_bytes(&7usize.to_le_bytes()).unwrap();
    let reason = kernel_std::env::EXIT_OK;
    let permission = kernel_std::env::Permission::FETCH | kernel_std::env::Permission::STORE;

    let _: kernel_api::env::TaskId = same_task(task);
    let _: kernel_api::env::TeamId = same_team(team);
    let _: kernel_api::env::ViewId = same_view(view);
    let _: kernel_api::env::PieToken = same_pie(pie);
    let _: kernel_api::env::FlatReference = same_reference(reference);
    let _: kernel_api::env::Reason = same_reason(reason);
    let _: kernel_api::env::Permission = same_permission(permission);
}
