#[path = "../src/envcall.rs"]
mod envcall;
#[path = "../src/fail.rs"]
mod fail;

use proc_macro2::TokenStream;
use quote::quote;

fn rejected(options: TokenStream, body: TokenStream, message: &str) {
    let output = envcall::expand(options, body).to_string();
    assert!(
        output.contains("compile_error"),
        "unexpected success: {output}"
    );
    assert!(output.contains(message), "wrong diagnostic: {output}");
}

#[test]
fn a_declaration_emits_contracts_and_dispatch_but_no_execution() {
    let output = envcall::expand(
        quote!(class = 4, call = ClockCall),
        quote! {
            pub mod clock {
                #[operation(slot = 0, output = usize, infallible)]
                #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                pub struct Ticks;
                #[operation(slot = 1, nonreturning)]
                #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                pub struct Stop;
            }
        },
    );
    let file: syn::File = syn::parse2(output.clone()).unwrap();
    let syn::Item::Mod(module) = &file.items[0] else {
        panic!()
    };
    assert!(
        module
            .content
            .as_ref()
            .unwrap()
            .1
            .iter()
            .all(|item| matches!(
                item,
                syn::Item::Struct(_)
                    | syn::Item::Enum(_)
                    | syn::Item::Impl(_)
                    | syn::Item::Const(_)
            ))
    );
    let text = output.to_string();
    for name in [
        "Operation",
        "RegisterCall",
        "KernelCall",
        "TerminalCall",
        "ClockCall",
    ] {
        assert!(text.contains(name));
    }
    assert!(!text.contains("KernelCall for Stop"));
    assert!(!text.contains("macro_rules"));
    assert!(!text.contains("unsafe"));
    let dispatch = module
        .content
        .as_ref()
        .unwrap()
        .1
        .iter()
        .find_map(|item| {
            let syn::Item::Impl(item) = item else {
                return None;
            };
            let syn::Type::Path(path) = item.self_ty.as_ref() else {
                return None;
            };
            (item.trait_.is_none() && path.path.is_ident("ClockCall")).then_some(item)
        })
        .unwrap();
    let methods: Vec<_> = dispatch
        .items
        .iter()
        .filter_map(|item| {
            let syn::ImplItem::Fn(method) = item else {
                return None;
            };
            Some(method.sig.ident.to_string())
        })
        .collect();
    assert_eq!(methods, ["from_wire"]);
}

#[test]
fn errors_and_operations_are_validated_together() {
    rejected(
        quote!(class = 2),
        quote! { mod api {
            #[operation(slot = 0, output = (), error = Missing)] pub struct Release;
        }},
        "error must name",
    );
    rejected(
        quote!(class = 2),
        quote! { mod api {
            #[error_domain] pub enum Failure { Denied = -2 }
            #[operation(slot = 0, output = (), error = Failure)] pub struct Release;
        }},
        "-1",
    );
    rejected(
        quote!(),
        quote! { mod api {
            #[error_domain] pub enum Failure { #[busy] A = -1, #[busy] B = -2 }
        }},
        "busy",
    );
}

#[test]
fn slots_and_classes_are_explicit_unique_and_bounded() {
    rejected(
        quote!(),
        quote! {mod api {#[operation(slot=0, output=(), infallible)] pub struct A;}},
        "require class",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(output=(), infallible)] pub struct A;}},
        "explicit slot",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {
            #[operation(slot=0, output=(), infallible)] pub struct A;
            #[operation(slot=0, output=(), infallible)] pub struct B;
        }},
        "duplicate call slot",
    );
    rejected(
        quote!(class = 4294967296u64),
        quote! {mod api {}},
        "too large",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=4294967296u64,output=(),infallible)] pub struct A;}},
        "too large",
    );
}

#[test]
fn contradictory_or_repeated_contract_options_are_rejected() {
    for options in [
        quote!(slot = 0, output = (), error = Fail, infallible),
        quote!(slot = 0, output = (), nonreturning),
        quote!(slot = 0, nonreturning, reply = 3),
        quote!(slot = 0, output = ()),
        quote!(slot = 0, infallible),
    ] {
        let output = envcall::expand(
            quote!(class = 0),
            quote! {mod api {#[operation(#options)] pub struct A;}},
        )
        .to_string();
        assert!(output.contains("compile_error"), "{output}");
    }
    rejected(
        quote!(class = 0, class = 1),
        quote! {mod api {}},
        "duplicate option",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,slot=1,output=(),infallible)] pub struct A;}},
        "duplicate option",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible,reply=4)] pub struct A;}},
        "reply must be",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible,typo)] pub struct A;}},
        "unknown operation option",
    );
    rejected(
        quote!(clas = 0),
        quote! {mod api {}},
        "unknown envcall option",
    );
}

#[test]
fn unsupported_shapes_report_diagnostics_instead_of_panicking() {
    rejected(
        quote!(),
        quote!(
            mod api;
        ),
        "inline module",
    );
    rejected(
        quote!(),
        quote!(
            mod api {}
        ),
        "error-only declaration",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible)] pub struct A<T>{pub value:T}}},
        "cannot be generic",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible)] pub struct A(pub usize);}},
        "named fields",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible)] pub struct A{value:usize}}},
        "inputs must be public",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[operation(slot=0,output=(),infallible)] #[operation(slot=1)] pub struct A;}},
        "duplicate #[operation]",
    );
    rejected(
        quote!(class = 0),
        quote! {mod api {#[cfg(any())] #[operation(slot=0,output=(),infallible)] pub struct A;}},
        "only doc and derive",
    );
}

#[test]
fn auxiliary_error_domains_need_no_fake_call_class() {
    let output = envcall::expand(
        quote!(),
        quote! {pub mod errors {
            #[error_domain] pub enum EventFail { Denied=-1, #[busy] Busy=-2 }
        }},
    )
    .to_string();
    assert!(!output.contains("compile_error"));
    assert!(output.contains("FailCode"));
    assert!(!output.contains("RegisterCall"));
}
