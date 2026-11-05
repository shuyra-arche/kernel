//! `#[derive(Envcall)]` —— **环境调用枚举**的载荷 codec（方案 3，typed payload）。
//!
//! 输入一个带载荷的调用枚举，输出：
//!   * `slot(&self) -> usize`       —— 调用号（`#[call(class = N)]` 的 class << 32 | 操作号）
//!   * `pack(&self) -> [usize; 6]`  —— 字段按声明顺序 wire 化（`Wire::pack`）
//!   * `from_wire(slot, &[usize; 6])` —— 按 slot 取 variant，逐字段 `Wire::unpack`
//!   * `Ret` 枚举                   —— 每个标 `#[ret(T)]` 的 variant 一个载荷变体
//!   * `decode_return(self, (a0,a1,a2))` —— 无副作用的返回解码
//!   * 隐藏生成宏 —— Client 指定 transport 后才生成每格一个精确签名的入口
//!
//! **两种返回宽度**：`#[ret(T)]` 走 `wire::FromPair`（读 `a0`/`a1`），`#[ret3(T)]` 走
//! `wire::FromTriple`（读 `a0..a2`）。宽度是**那一格载荷自己的事实**：一对寄存器说不完的
//! 才标 `ret3`（今天只有 `PieCall::Collect`），其余四十八格一个字不改。
//!
//! 通用性：`slot/pack/unpack` 与 `Ret` 只依赖 `Wire`（不绑 env 错误/汇编），sbi 等
//! Client 生成体经 `$crate` 指向 API owner，只有 transport 路径来自 Client。
//!
//! 本文件是该宏的全部：解析（[`ret_type`] / [`ret_wide_type`] / [`class`]）、变体那一格
//! （[`Variant`]）与展开（[`expand`]）——**与另外两个宏一行都不共享**。

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Lit, Type, parse2};

/// 展开见文件头。
pub fn expand(input: TokenStream2) -> TokenStream2 {
    let ast = match parse2::<DeriveInput>(input) {
        Ok(ast) => ast,
        Err(e) => return e.to_compile_error(),
    };
    let name = &ast.ident;
    let (class, fail_ty) = match call_attrs(&ast.attrs) {
        Ok(c) => c,
        Err(e) => return e.to_compile_error(),
    };
    let vols = match variants(&ast) {
        Ok(v) => v,
        Err(e) => return e.to_compile_error(),
    };
    // **不变式**：会失败却没有词表的类，是那张声明的错——在这里当场报，不往后传。
    if fail_ty.is_none() && vols.iter().any(|v| !v.infallible && !v.manual) {
        return syn::Error::new_spanned(
            &ast,
            "这一类的某格会失败，但类上没标 `fail = XxxFail`（或该格应标 `#[infallible]`）",
        )
        .to_compile_error();
    }
    let ret_name = Ident::new(&format!("{}Ret", name), proc_macro2::Span::call_site());

    let mod_name = Ident::new(
        &class_module(&name.to_string()),
        proc_macro2::Span::call_site(),
    );
    let client_macro = Ident::new(
        &format!("__kernel_client_{}", mod_name),
        proc_macro2::Span::call_site(),
    );
    let gen_fns: Vec<TokenStream2> = vols
        .iter()
        .filter(|v| !v.manual)
        .map(|v| gen_fn(name, v, fail_ty.as_ref()))
        .collect();
    let gen_mod = quote! {
        /// Instantiate the typed client from this same ABI declaration.
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #client_macro {
            ($transport:path) => {
                pub mod #mod_name {
                    use $crate::*;
                    #(#gen_fns)*
                }
            };
        }
    };

    let slot_arms: Vec<_> = vols
        .iter()
        .map(|v| {
            let i = v.slot;
            let form = v.shape(None);
            quote! { #form => ( #class << 32 ) | #i }
        })
        .collect();

    let pack_arms: Vec<_> = vols
        .iter()
        .map(|v| {
            let binds = v.binds();
            let form = v.shape(Some(&binds));
            if v.is_unit() {
                quote! { #form => [0usize; 6] }
            } else {
                let typs = v.types();
                let packs = binds
                    .iter()
                    .zip(typs.iter())
                    .map(|(b, t)| {
                        quote! {
                            <#t as crate::wire::Wire>::pack(&#b, &mut s, &mut i);
                        }
                    })
                    .collect::<Vec<_>>();
                quote! {
                    #form => {
                        let mut s = [0usize; 6];
                        let mut i = 0usize;
                        #(#packs)*
                        s
                    }
                }
            }
        })
        .collect();

    let unpack_arms: Vec<_> = vols
        .iter()
        .map(|v| {
            let i = v.slot;
            let binds = v.binds();
            let form = v.shape(Some(&binds));
            if v.is_unit() {
                quote! { #i => Ok(#form) }
            } else {
                let typs = v.types();
                let unpacks = binds
                    .iter()
                    .zip(typs.iter())
                    .map(|(b, t)| {
                        quote! {
                            let #b: #t = <#t as crate::wire::Wire>::unpack(&regs, &mut i)?;
                        }
                    })
                    .collect::<Vec<_>>();
                quote! {
                    #i => {
                        let mut i = 0usize;
                        #(#unpacks)*
                        Ok(#form)
                    }
                }
            }
        })
        .collect();

    let ret_variants: Vec<_> = vols
        .iter()
        .map(|v| {
            let (id, ty) = (&v.ident, &v.ret);
            quote! {
                #id(#ty)
            }
        })
        .collect();

    let distill_arms: Vec<_> = vols
        .iter()
        .map(|v| {
            let i = v.slot;
            let (id, ty) = (&v.ident, &v.ret);
            if v.wide {
                quote! {
                    #i => #ret_name::#id(<#ty as crate::wire::FromTriple>::from_triple(v0, v1, v2))
                }
            } else {
                quote! {
                    #i => #ret_name::#id(<#ty as crate::wire::FromPair>::from_pair(v0, v1))
                }
            }
        })
        .collect();

    // 返回解码的错类型跟着这一类的词表走；无词表的类不返 Result。
    let call_ret: TokenStream2 = match &fail_ty {
        Some(f) => quote! { Result<#ret_name, erra::Error<#f>> },
        None => quote! { #ret_name },
    };
    let call_ok: TokenStream2 = match &fail_ty {
        Some(_) => quote! {
            Ok(match slot & 0xFFFF_FFFF {
                #(#distill_arms),*,
                _ => unreachable!(),
            })
        },
        None => quote! {
            match slot & 0xFFFF_FFFF {
                #(#distill_arms),*,
                _ => unreachable!(),
            }
        },
    };
    let call_on_err: TokenStream2 = match &fail_ty {
        Some(f) => quote! {
            match <#f>::of_code(v0 as isize) {
                Some(f) => Err(crate::ecall::make_fail(f)),
                None => unreachable!("内核答了表外的码：{}", v0),
            }
        },
        None => quote! { unreachable!("这一类没有词表，内核却答了负码：{}", v0) },
    };
    let fallible_slots: Vec<_> = vols
        .iter()
        .filter(|v| !v.infallible)
        .map(|v| v.slot)
        .collect();
    let has_failure = if fail_ty.is_none() || fallible_slots.is_empty() {
        quote! { false }
    } else {
        quote! { matches!(slot & 0xFFFF_FFFF, #(#fallible_slots)|*) }
    };

    let expanded = quote! {
        impl #name {
            #[inline]
            pub const fn slot(&self) -> usize {
                match self {
                    #(#slot_arms),*
                }
            }

            #[inline]
            pub fn pack(&self) -> [usize; 6] {
                match self {
                    #(#pack_arms),*
                }
            }

            /// 由调用号 + 寄存器组解码回本枚举（校验式，非法位 → Err）。
            #[inline]
            pub fn from_wire(slot: usize, regs: &[usize; 6]) -> Result<Self, crate::wire::Decode> {
                let index = (slot & 0xFFFF_FFFF) as usize;
                match index {
                    #(#unpack_arms),*,
                    _ => Err(crate::wire::Decode::BadSlot),
                }
            }
        }

        /// 本域调用的结果枚举（R3：每个原语一个锁定的返回类型）。
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum #ret_name {
            #(#ret_variants),*
        }

        impl #name {
            /// Decode return registers without performing an environment call.
            #[inline]
            pub fn decode_return(self, (v0, v1, v2): (usize, usize, usize)) -> #call_ret {
                let slot = self.slot();
                if #has_failure && (v0 as isize) < 0 {
                    #call_on_err
                } else {
                    #call_ok
                }
            }
        }

        #gen_mod
    };

    expanded
}

/// 一枚变体：**名字、字段、返回载荷、返回宽度、以及"这一格有没有失败面/是不是手写"**
/// 绑在一起。
///
/// 一枚变体的字段表——它们原本是四个并行数组（`vs` / `flds` / `rets` / `wides`），
/// 靠下标对齐——每一处用它的地方都要写 `&vs[i]` / `&flds[i]` / `rets[i].clone()`，
/// 下标写错一格**编得过**。绑成 struct 之后下标错当场编不过。
struct Variant {
    slot: usize,
    ident: Ident,
    fields: Fields,
    /// 这一格的返回载荷：`#[ret(T)]` 读 `a0`/`a1`，`#[ret3(T)]` 读 `a0..a2`。
    ///
    /// **不是 `Option`**：`Ret` 枚举一个变体一格载荷 ⇒ "这一格没标返回"是那张表的错，
    /// 在 [`variants`] 里当场报、不往后传 `None`。
    ret: Type,
    /// 标的是 `#[ret3(T)]` ⇒ 蒸馏走 `FromTriple` 而不是 `FromPair`。
    wide: bool,
    /// `#[infallible]`：**这一格没有失败域** ⇒ 生成的入口返裸载荷，不返 `Result`。
    infallible: bool,
    /// `#[manual]`：这一格的手写包装有形状要求（今天只有 `Reap`：发散）⇒ 不生成入口。
    manual: bool,
}

impl Variant {
    /// 字段类型，按声明顺序。
    fn types(&self) -> Vec<Type> {
        match &self.fields {
            Fields::Named(named) => named.named.iter().map(|f| f.ty.clone()).collect(),
            Fields::Unnamed(unnamed) => unnamed.unnamed.iter().map(|f| f.ty.clone()).collect(),
            Fields::Unit => Vec::new(),
        }
    }

    /// 字段名，按声明顺序（无名那一族现造 `f0`/`f1`…——名字只在展开体里用，不落到用户面）。
    fn binds(&self) -> Vec<Ident> {
        match &self.fields {
            Fields::Named(named) => named
                .named
                .iter()
                .map(|f| f.ident.clone().unwrap())
                .collect(),
            Fields::Unnamed(unnamed) => (0..unnamed.unnamed.len())
                .map(|i| Ident::new(&format!("f{i}"), proc_macro2::Span::call_site()))
                .collect(),
            Fields::Unit => Vec::new(),
        }
    }

    fn is_unit(&self) -> bool {
        matches!(self.fields, Fields::Unit)
    }

    /// **变体那一格的唯一一处拼法**：匹配与造值**逐字同形**，故只有这一个函数。
    ///
    /// `binds = None` ⇒ 字段位写 `..`：只匹配、不绑名字（不绑就不会长出"未使用的变量"）。
    /// 这件事的拼法**只此一处**——`pat_for` / `expr_for` / `slot(&self)` 三处各写一份
    /// 的形式已收拢到这里。
    fn shape(&self, binds: Option<&[Ident]>) -> TokenStream2 {
        let v = &self.ident;
        match (&self.fields, binds) {
            (Fields::Unit, _) => quote! { Self::#v },
            (Fields::Unnamed(_), Some(b)) => quote! { Self::#v(#(#b),*) },
            (Fields::Unnamed(_), None) => quote! { Self::#v(..) },
            (Fields::Named(_), Some(b)) => quote! { Self::#v { #(#b),* } },
            (Fields::Named(_), None) => quote! { Self::#v { .. } },
        }
    }
}

/// 解析 `#[ret(T)]` 属性里的返回类型。
fn ret_type(attrs: &[Attribute]) -> syn::Result<Option<Type>> {
    for attr in attrs {
        if attr.path().is_ident("ret") {
            let ty = attr.parse_args::<Type>()?;
            return Ok(Some(ty));
        }
    }
    Ok(None)
}

/// 解析 `#[ret3(T)]` 属性里的返回类型（**宽返回那一格**：读 `a0..a2`）。
///
/// 与 [`ret_type`] 分成两个函数、而不是一个函数认两种拼法：**一格载荷最多有一个返回
/// 宽度**，两处各自只认自己那个属性名，撞上了（同一 variant 两个都标）由
/// [`variants`] 当场报错，而不是让后一个静默覆盖前一个。
fn ret_wide_type(attrs: &[Attribute]) -> syn::Result<Option<Type>> {
    for attr in attrs {
        if attr.path().is_ident("ret3") {
            let ty = attr.parse_args::<Type>()?;
            return Ok(Some(ty));
        }
    }
    Ok(None)
}

/// 解析 `#[call(class = N, fail = XxxFail)]`：class 必给，`fail` 可缺（缺 ⇒ 这一类的
/// 每格入口在过渡期返 `EnvResult`）。
///
/// **`fail` 是"这一类的失败词汇"**：词表由 `#[derive(Fail)]` 生成，码按域自持；
/// 类里每一格的入口都用它当错类型（`#[infallible]` 的格除外）。
fn call_attrs(attrs: &[Attribute]) -> syn::Result<(usize, Option<Type>)> {
    for attr in attrs {
        if attr.path().is_ident("call") {
            let mut class = None;
            let mut fail = None;
            attr.parse_nested_meta(|nested| {
                if nested.path.is_ident("class") {
                    let lit: Lit = nested.value()?.parse()?;
                    if let Lit::Int(i) = lit {
                        class = Some(i.base10_parse::<usize>()?);
                    }
                } else if nested.path.is_ident("fail") {
                    fail = Some(nested.value()?.parse::<Type>()?);
                }
                Ok(())
            })?;
            return match class {
                Some(c) => Ok((c, fail)),
                None => Err(syn::Error::new_spanned(
                    attr,
                    "expected #[call(class = N, fail = XxxFail)]",
                )),
            };
        }
    }
    Err(syn::Error::new_spanned(
        &attrs[0],
        "expected #[call(class = N, fail = XxxFail)] on the enum",
    ))
}

/// 一格有没有标某个变体级属性（`#[infallible]` / `#[manual]`）。
fn marked(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

/// 从 `DeriveInput` 提取变体列表；三条当场报错，都落在**那一格的名字**上：不是枚举、
/// 两种返回宽度都标了、**这一格没标返回**。
fn variants(ast: &DeriveInput) -> syn::Result<Vec<Variant>> {
    let data = match &ast.data {
        Data::Enum(e) => e,
        _ => return Err(syn::Error::new_spanned(ast, "Envcall only supports enums")),
    };
    let mut slots = std::collections::BTreeSet::new();
    let explicit = data.variants.iter().any(|v| marked(&v.attrs, "slot"));
    data.variants
        .iter()
        .enumerate()
        .map(|(index, v)| {
            let slot = match v.attrs.iter().find(|a| a.path().is_ident("slot")) {
                Some(attr) => attr.parse_args::<syn::LitInt>()?.base10_parse::<usize>()?,
                None if explicit => return Err(syn::Error::new_spanned(v, "every variant needs #[slot(N)] when explicit slots are used")),
                None => index,
            };
            if slot > u32::MAX as usize || !slots.insert(slot) {
                return Err(syn::Error::new_spanned(v, "duplicate or out-of-range call slot"));
            }
            let (narrow, wide) = (ret_type(&v.attrs)?, ret_wide_type(&v.attrs)?);
            let (ret, is_wide) = match (narrow, wide) {
                (Some(_), Some(_)) => {
                    return Err(syn::Error::new_spanned(
                        &v.ident,
                        "改一格载荷的返回宽度：`#[ret(T)]` 与 `#[ret3(T)]` 只能标一个",
                    ));
                }
                (None, None) => {
                    return Err(syn::Error::new_spanned(
                        &v.ident,
                        "这一格没有返回载荷：标 `#[ret(T)]`（读 `a0`/`a1`）或 `#[ret3(T)]`（读 `a0..a2`）",
                    ));
                }
                (Some(t), None) => (t, false),
                (None, Some(t)) => (t, true),
            };
            Ok(Variant {
                slot,
                ident: v.ident.clone(),
                fields: v.fields.clone(),
                ret,
                wide: is_wide,
                infallible: marked(&v.attrs, "infallible"),
                manual: marked(&v.attrs, "manual"),
            })
        })
        .collect()
}

/// 一格一个入口（住本类那个生成模块里）。
///
/// 签名 = **这一格自己的载荷**：字段按声明顺序成为参数；`#[infallible]` 的格返裸载荷，
/// 其余返 `Result<载荷, erra::Error<域词表>>`（类没标 `fail` 的过渡期返 `EnvResult`）。
fn gen_fn(owner: &Ident, v: &Variant, fail: Option<&Type>) -> TokenStream2 {
    let fname = Ident::new(&snake_case(&v.ident.to_string()), v.ident.span());
    let variant = &v.ident;
    let ret_owner = Ident::new(&format!("{}Ret", owner), owner.span());
    let binds = v.binds();
    let types = v.types();
    let ctor = match &v.fields {
        Fields::Unit => quote! { #owner::#variant },
        Fields::Unnamed(_) => quote! { #owner::#variant(#(#binds),*) },
        Fields::Named(_) => quote! { #owner::#variant { #(#binds),* } },
    };
    let ret = &v.ret;
    let is_unit = matches!(ret, Type::Tuple(t) if t.elems.is_empty());
    let (sig_ret, tail) = match (fail, v.infallible) {
        (None, _) => {
            let sig = if is_unit {
                quote! {}
            } else {
                quote! { -> #ret }
            };
            (
                sig,
                quote! {
                    match this.decode_return(raw) {
                        #ret_owner::#variant(value) => value,
                        _ => unreachable!("return decoder changed the request variant"),
                    }
                },
            )
        }
        (Some(_), true) => {
            let sig = if is_unit {
                quote! {}
            } else {
                quote! { -> #ret }
            };
            (
                sig,
                quote! {
                    match this.decode_return(raw) {
                        Ok(#ret_owner::#variant(value)) => value,
                        Err(_) => panic!("infallible environment call returned a failure"),
                        _ => unreachable!("return decoder changed the request variant"),
                    }
                },
            )
        }
        (Some(f), false) => (
            quote! { -> Result<#ret, $crate::__CallError<#f>> },
            quote! {
                match this.decode_return(raw) {
                    Ok(#ret_owner::#variant(value)) => Ok(value),
                    Err(error) => Err(error),
                    _ => unreachable!("return decoder changed the request variant"),
                }
            },
        ),
    };
    quote! {
        #[inline]
        pub fn #fname(#(#binds: #types),*) #sig_ret {
            let this = #ctor;
            let raw = unsafe { $transport(this.slot(), this.pack()) };
            #tail
        }
    }
}

/// `UnsealHole` → `unseal_hole`（生成入口的名字由变体名定）。
///
/// **撞上关键字的加下划线**（`Await` → `await_`，与手写那一层的 `mail::await_` 同拼法）——
/// 否则生成的是 `pub fn await`，编都编不过。
fn snake_case(name: &str) -> String {
    let cs: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, c) in cs.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_small =
                i > 0 && (cs[i - 1].is_ascii_lowercase() || cs[i - 1].is_ascii_digit());
            let next_small = i + 1 < cs.len() && cs[i + 1].is_ascii_lowercase();
            if i > 0 && (prev_small || next_small) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// 生成入口要避开的 Rust 关键字（**认到一个加一个**，不是全集洁癖）。
const KEYWORDS: [&str; 12] = [
    "as", "await", "box", "do", "fn", "in", "let", "loop", "match", "move", "ref", "type",
];

/// `RoomCall` → `room`：**每类一个生成模块**，跨类的同名变体（`Wait` 在 Room/Mail/Unit）
/// 靠它隔离。
fn class_module(name: &str) -> String {
    name.strip_suffix("Call")
        .unwrap_or(name)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_slots_follow_attributes_instead_of_declaration_order() {
        let input = syn::parse_quote! {
            enum Calls {
                #[slot(9)] #[ret(())] Last,
                #[slot(2)] #[ret(())] First,
            }
        };
        let parsed = variants(&input).unwrap();
        assert_eq!(parsed.iter().map(|v| v.slot).collect::<Vec<_>>(), [9, 2]);
    }

    #[test]
    fn explicit_and_implicit_slots_cannot_be_mixed() {
        let input = syn::parse_quote! {
            enum Calls { #[slot(9)] #[ret(())] A, #[ret(())] B }
        };
        assert!(variants(&input).is_err());
    }

    #[test]
    fn duplicate_and_out_of_range_slots_are_rejected() {
        let duplicate = syn::parse_quote! {
            enum Calls { #[slot(1)] #[ret(())] A, #[slot(1)] #[ret(())] B }
        };
        assert!(variants(&duplicate).is_err());
        let overflow = syn::parse_quote! {
            enum Calls { #[slot(4294967296)] #[ret(())] A }
        };
        assert!(variants(&overflow).is_err());
    }
}
