//! One declaration owns request shapes, success types, error domains and ABI.

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, Fields, Ident, Item, ItemMod, ItemStruct, LitInt, Type, parse2};

pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_checked(attr, item).unwrap_or_else(syn::Error::into_compile_error)
}

fn marker(attrs: &mut Vec<Attribute>, name: &str) -> syn::Result<Option<Attribute>> {
    let mut found = None;
    let mut kept = Vec::new();
    for attr in attrs.drain(..) {
        if attr.path().is_ident(name) {
            if found.is_some() {
                return Err(syn::Error::new_spanned(
                    attr,
                    format!("duplicate #[{name}]"),
                ));
            }
            found = Some(attr);
        } else {
            kept.push(attr);
        }
    }
    *attrs = kept;
    Ok(found)
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    meta: &syn::meta::ParseNestedMeta,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error("duplicate option"));
    }
    *slot = Some(value);
    Ok(())
}

fn declaration_attrs(attrs: &[Attribute]) -> syn::Result<()> {
    for attr in attrs {
        if !attr.path().is_ident("doc") && !attr.path().is_ident("derive") {
            return Err(syn::Error::new_spanned(
                attr,
                "only doc and derive attributes are supported on ABI declarations",
            ));
        }
    }
    Ok(())
}

struct Operation {
    item: ItemStruct,
    slot: u32,
    output: Option<Type>,
    error: Option<Ident>,
    reply: u32,
    nonreturning: bool,
}

fn operation(mut item: ItemStruct, attr: Attribute) -> syn::Result<Operation> {
    declaration_attrs(&item.attrs)?;
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "ABI operations cannot be generic",
        ));
    }
    if !matches!(item.vis, syn::Visibility::Public(_)) {
        return Err(syn::Error::new_spanned(
            &item,
            "ABI operations must be public",
        ));
    }
    if !matches!(item.fields, Fields::Named(_) | Fields::Unit) {
        return Err(syn::Error::new_spanned(
            &item.fields,
            "use named fields or a unit operation",
        ));
    }
    for field in &mut item.fields {
        if !matches!(field.vis, syn::Visibility::Public(_)) {
            return Err(syn::Error::new_spanned(
                field,
                "operation inputs must be public",
            ));
        }
        for attr in &field.attrs {
            if !attr.path().is_ident("doc") {
                return Err(syn::Error::new_spanned(
                    attr,
                    "only doc attributes are supported on operation inputs",
                ));
            }
        }
    }
    let (mut slot, mut output, mut error, mut reply, mut infallible, mut nonreturning) =
        (None, None, None, None, None, None);
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("slot") {
            let value = meta.value()?.parse::<LitInt>()?.base10_parse::<u32>()?;
            set_once(&mut slot, value, &meta)
        } else if meta.path.is_ident("output") {
            let value = meta.value()?.parse::<Type>()?;
            set_once(&mut output, value, &meta)
        } else if meta.path.is_ident("error") {
            let value = meta.value()?.parse::<Ident>()?;
            set_once(&mut error, value, &meta)
        } else if meta.path.is_ident("reply") {
            let value = meta.value()?.parse::<LitInt>()?.base10_parse::<u32>()?;
            set_once(&mut reply, value, &meta)
        } else if meta.path.is_ident("infallible") {
            set_once(&mut infallible, true, &meta)
        } else if meta.path.is_ident("nonreturning") {
            set_once(&mut nonreturning, true, &meta)
        } else {
            Err(meta.error("unknown operation option"))
        }
    })?;
    let slot =
        slot.ok_or_else(|| syn::Error::new_spanned(&attr, "an explicit slot is required"))?;
    let terminal = nonreturning.is_some();
    if terminal {
        if output.is_some() || error.is_some() || reply.is_some() || infallible.is_some() {
            return Err(syn::Error::new_spanned(
                &attr,
                "nonreturning operations have no output, error or reply decoder",
            ));
        }
    } else if output.is_none() || error.is_some() == infallible.is_some() {
        return Err(syn::Error::new_spanned(
            &attr,
            "returning operations require output and exactly one of error or infallible",
        ));
    }
    let reply = reply.unwrap_or(2);
    if reply != 2 && reply != 3 {
        return Err(syn::Error::new_spanned(
            &attr,
            "reply must be 2 or 3 registers",
        ));
    }
    Ok(Operation {
        item,
        slot,
        output,
        error,
        reply,
        nonreturning: terminal,
    })
}

fn expand_checked(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let mut module: ItemMod = parse2(item)?;
    let (mut class, mut call) = (None, None);
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("class") {
            let value = u32::from(meta.value()?.parse::<LitInt>()?.base10_parse::<u16>()?);
            set_once(&mut class, value, &meta)
        } else if meta.path.is_ident("call") {
            let value = meta.value()?.parse::<Ident>()?;
            set_once(&mut call, value, &meta)
        } else {
            Err(meta.error("unknown envcall option"))
        }
    });
    syn::parse::Parser::parse2(parser, attr)?;
    let (_, items) = module
        .content
        .take()
        .ok_or_else(|| syn::Error::new_spanned(&module, "envcall requires an inline module"))?;
    let mut retained = Vec::new();
    let mut operations = Vec::new();
    let mut errors = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for item in items {
        match item {
            Item::Enum(mut item) => {
                let Some(attr) = marker(&mut item.attrs, "error_domain")? else {
                    return Err(syn::Error::new_spanned(
                        item,
                        "enums in an envcall declaration require #[error_domain]",
                    ));
                };
                if !matches!(attr.meta, syn::Meta::Path(_)) {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "error_domain takes no options",
                    ));
                }
                declaration_attrs(&item.attrs)?;
                if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
                    return Err(syn::Error::new_spanned(
                        &item.generics,
                        "error domains cannot be generic",
                    ));
                }
                if !matches!(item.vis, syn::Visibility::Public(_)) {
                    return Err(syn::Error::new_spanned(
                        &item,
                        "error domains must be public",
                    ));
                }
                if !names.insert(item.ident.to_string()) {
                    return Err(syn::Error::new_spanned(
                        &item.ident,
                        "duplicate ABI declaration name",
                    ));
                }
                errors.insert(item.ident.to_string());
                // The internal generator owns the Kernel failure convention.
                let generated = crate::fail::expand(&item)?;
                for variant in &mut item.variants {
                    if let Some(busy) = marker(&mut variant.attrs, "busy")? {
                        if !matches!(busy.meta, syn::Meta::Path(_)) {
                            return Err(syn::Error::new_spanned(busy, "busy takes no options"));
                        }
                    }
                    for attr in &variant.attrs {
                        if !attr.path().is_ident("doc") {
                            return Err(syn::Error::new_spanned(
                                attr,
                                "only doc and busy attributes are supported on failures",
                            ));
                        }
                    }
                }
                retained.push(quote!(#item #generated));
            }
            Item::Struct(mut item) => {
                let attr = marker(&mut item.attrs, "operation")?.ok_or_else(|| {
                    syn::Error::new_spanned(&item, "operation structs require #[operation(...)]")
                })?;
                let op = operation(item, attr)?;
                if !slots.insert(op.slot) {
                    return Err(syn::Error::new_spanned(
                        &op.item.ident,
                        "duplicate call slot",
                    ));
                }
                if !names.insert(op.item.ident.to_string()) {
                    return Err(syn::Error::new_spanned(
                        &op.item.ident,
                        "duplicate ABI declaration name",
                    ));
                }
                operations.push(op);
            }
            Item::Use(item) => retained.push(quote!(#item)),
            _ => {
                return Err(syn::Error::new_spanned(
                    item,
                    "envcall modules contain imports, error domains and operations only",
                ));
            }
        }
    }
    if operations.is_empty() {
        if class.is_some() || call.is_some() || errors.is_empty() {
            return Err(syn::Error::new_spanned(
                &module.ident,
                "an error-only declaration needs error domains and no class or call options",
            ));
        }
    } else {
        let class = class.ok_or_else(|| {
            syn::Error::new_spanned(&module.ident, "operation declarations require class")
        })?;
        let call = call.unwrap_or_else(|| format_ident!("Call"));
        if names.contains(&call.to_string()) {
            return Err(syn::Error::new_spanned(
                &call,
                "call enum name conflicts with an ABI declaration",
            ));
        }
        for op in &operations {
            if let Some(error) = &op.error {
                if !errors.contains(&error.to_string()) {
                    return Err(syn::Error::new_spanned(
                        error,
                        "error must name an error_domain in this declaration",
                    ));
                }
            }
            retained.push(emit_operation(op, class, &call));
        }
        retained.push(emit_call(&operations, class, &call));
    }
    let attrs = &module.attrs;
    let vis = &module.vis;
    let ident = &module.ident;
    Ok(quote! { #(#attrs)* #vis mod #ident { #(#retained)* } })
}

fn emit_operation(op: &Operation, class: u32, call: &Ident) -> TokenStream {
    let item = &op.item;
    let name = &item.ident;
    let slot = ((class as u64) << 32) | u64::from(op.slot);
    let fields: Vec<_> = item.fields.iter().collect();
    let ids: Vec<_> = fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect();
    let locals: Vec<_> = (0..fields.len())
        .map(|i| format_ident!("__input_{i}"))
        .collect();
    let types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let construct = if matches!(item.fields, Fields::Unit) {
        quote!(Self)
    } else {
        quote!(Self { #(#ids: #locals),* })
    };
    let enum_value = if matches!(item.fields, Fields::Unit) {
        quote!(#call::#name)
    } else {
        quote!(#call::#name { #(#ids: _value.#ids),* })
    };
    let behavior = if op.nonreturning {
        quote!(impl crate::abi::operation::TerminalCall for #name {})
    } else {
        let output = op.output.as_ref().unwrap();
        let success = if op.reply == 3 {
            quote!(<#output as crate::abi::reply::FromTriple>::from_triple(v0, v1, _v2))
        } else {
            quote!(<#output as crate::abi::reply::FromPair>::from_pair(v0, v1))
        };
        let (error, decode_error) = match &op.error {
            Some(error) => (
                quote!(erra::Error<#error>),
                quote! {
                    if (v0 as isize) < 0 {
                        return Err(crate::abi::error::make_fail(
                            #error::of_code(v0 as isize).expect("Kernel returned an unknown failure code")
                        ));
                    }
                },
            ),
            None => (quote!(core::convert::Infallible), quote!()),
        };
        quote! {
            impl crate::abi::operation::Operation for #name {
                type Output = #output;
                type Error = #error;
            }
            impl crate::abi::operation::KernelCall for #name {
                #[inline]
                fn decode_return((v0, v1, _v2): (usize, usize, usize)) -> Result<Self::Output, Self::Error> {
                    #decode_error
                    Ok(#success)
                }
            }
        }
    };
    let pack = if fields.is_empty() {
        quote!([0usize; 6])
    } else {
        quote! {
            let mut registers = [0usize; 6];
            let mut cursor = 0usize;
            #(<#types as crate::abi::register::Wire>::pack(&self.#ids, &mut registers, &mut cursor);)*
            registers
        }
    };
    let unpack = if fields.is_empty() {
        quote!(Ok(#construct))
    } else {
        quote! {
            let mut cursor = 0usize;
            #(let #locals = <#types as crate::abi::register::Wire>::unpack(_registers, &mut cursor)?;)*
            Ok(#construct)
        }
    };
    quote! {
        #item
        const _: () = assert!(usize::BITS >= 64, "Kernel register ABI requires a 64-bit target");
        const _: () = assert!(0usize #(+ <#types as crate::abi::register::Wire>::WORDS)* <= 6,
            "operation inputs exceed six ABI registers");
        impl crate::abi::operation::RegisterCall for #name {
            const SLOT: usize = crate::abi::PREFIX | #slot as usize;
            #[inline]
            fn pack(&self) -> [usize; 6] {
                #pack
            }
            #[inline]
            fn from_registers(_registers: &[usize; 6]) -> Result<Self, crate::abi::register::Decode> {
                #unpack
            }
        }
        impl From<#name> for #call {
            fn from(_value: #name) -> Self { #enum_value }
        }
        #behavior
    }
}

fn emit_call(operations: &[Operation], class: u32, name: &Ident) -> TokenStream {
    let mut variants = Vec::new();
    let mut decodes = Vec::new();
    for op in operations {
        let id = &op.item.ident;
        let docs: Vec<_> = op
            .item
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"))
            .collect();
        let ids: Vec<_> = op
            .item
            .fields
            .iter()
            .map(|field| field.ident.as_ref().unwrap())
            .collect();
        let types: Vec<_> = op.item.fields.iter().map(|field| &field.ty).collect();
        if matches!(op.item.fields, Fields::Unit) {
            variants.push(quote!(#(#docs)* #id));
        } else {
            variants.push(quote!(#(#docs)* #id { #(#ids: #types),* }));
        }
        let slot = op.slot;
        decodes.push(quote!(#slot => Ok(<#id as crate::abi::operation::RegisterCall>::from_registers(registers)?.into())));
    }
    quote! {
        /// Server-side dispatch view of the operations in this ABI domain.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum #name { #(#variants),* }
        impl #name {
            #[inline]
            pub fn from_wire(slot: usize, registers: &[usize; 6]) -> Result<Self, crate::abi::register::Decode> {
                if (slot >> 48) != crate::abi::VERSION as usize || ((slot >> 32) & 0xffff) != #class as usize {
                    return Err(crate::abi::register::Decode::BadSlot);
                }
                match slot as u32 {
                    #(#decodes),*,
                    _ => Err(crate::abi::register::Decode::BadSlot),
                }
            }
        }
    }
}
