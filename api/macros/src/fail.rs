//! Internal error-domain generator used only by the unified `envcall` macro.
//!
//! Kernel failure codes are consecutive negative values starting at -1 per
//! domain. One variant may carry `busy`. The enclosing declaration validates
//! the association between these domains and operations before emitting code.
use proc_macro2::{Literal, TokenStream as TokenStream2};
use quote::quote;
use syn::{Attribute, Expr, Fields, ItemEnum, Lit};

/// 展开见文件头。
pub fn expand(ast: &ItemEnum) -> syn::Result<TokenStream2> {
    let name = &ast.ident;
    if ast.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            ast,
            "空词表没有意义：一枚变体都没有",
        ));
    }

    // 域名词：`MemoryFail` → `memory`。
    let full = name.to_string();
    let domain = full.strip_suffix("Fail").unwrap_or(&full).to_lowercase();

    let mut idents = Vec::new();
    let mut codes = Vec::new();
    let mut busy = None;
    for (at, v) in ast.variants.iter().enumerate() {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                v,
                "失败词汇的变体不带字段：一枚变体就是一格码",
            ));
        }
        let want = -(at as isize) - 1;
        let got = v.discriminant.as_ref().and_then(|(_, e)| int_of(e));
        match got {
            Some(g) if g == want => {}
            Some(g) => {
                return Err(syn::Error::new_spanned(
                    v,
                    format!("这一格的号必须是 {want}（域内自 -1 起、按声明顺序连续），写的是 {g}"),
                ));
            }
            None => {
                return Err(syn::Error::new_spanned(
                    v,
                    format!("这一格要显式写号：`= {want}`（域内自 -1 起、按声明顺序连续）"),
                ));
            }
        }
        if has_busy(&v.attrs) {
            if busy.is_some() {
                return Err(syn::Error::new_spanned(
                    v,
                    "一个域只有一枚 `#[busy]`（「条件未就绪」是单数）",
                ));
            }
            busy = Some(&v.ident);
        }
        idents.push(&v.ident);
        codes.push(Literal::isize_unsuffixed(want));
    }

    let out = Literal::isize_unsuffixed(-(idents.len() as isize) - 1);
    let domain_lit = Literal::string(&domain);
    let busy_fn = busy.map(|b| {
        quote! {
            impl #name {
                /// 条件未就绪（`#[busy]` 那一枚）——非阻塞原语的可重试信号。
                pub const fn is_busy(self) -> bool {
                    matches!(self, Self::#b)
                }
            }
        }
    });

    Ok(quote! {
        impl #name {
            /// 域内判别值：**自 `-1` 起按声明顺序连续**（判别值即码）。
            pub const fn code(self) -> isize {
                self as isize
            }

            /// 码读回词汇；表外答 `None`（调用点当场 `unreachable!`：表外码 = 不变量破裂）。
            pub const fn of_code(code: isize) -> Option<Self> {
                match code {
                    #(#codes => Some(Self::#idents),)*
                    _ => None,
                }
            }
        }

        impl crate::FailCode for #name {
            fn code(self) -> isize {
                self.code()
            }
        }

        impl Clone for #name {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl Copy for #name {}

        impl core::fmt::Debug for #name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let name = match self {
                    #(Self::#idents => stringify!(#idents),)*
                };
                core::write!(f, "{}::{}", stringify!(#name), name)
            }
        }

        impl PartialEq for #name {
            fn eq(&self, other: &Self) -> bool {
                self.code() == other.code()
            }
        }

        impl Eq for #name {}

        impl core::fmt::Display for #name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                use core::fmt::Write as _;
                let name = match self {
                    #(Self::#idents => stringify!(#idents),)*
                };
                write!(f, "{}: {}", #domain_lit, name)
            }
        }

        #busy_fn

        /// 逐域锁死：号自 `-1` 起连续、逐枚读得回来、表外答 `None`。
        const _: () = {
            #(assert!(#name::#idents.code() == #codes);)*
            #(assert!(matches!(#name::of_code(#codes), Some(#name::#idents)));)*
            assert!(#name::of_code(0).is_none());
            assert!(#name::of_code(1).is_none());
            assert!(#name::of_code(#out).is_none());
        };
    })
}

/// `#[busy]` is consumed by the enclosing envcall declaration.
fn has_busy(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("busy"))
}

/// 判别值里的整数——**`-1` 在语法上是"一元负号 + 字面量"**，不是 `Expr::Lit`。
fn int_of(e: &Expr) -> Option<isize> {
    match e {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Int(n), ..
        }) => n.base10_parse::<isize>().ok(),
        Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr,
            ..
        }) => int_of(expr).map(|v| -v),
        _ => None,
    }
}
