// expect: E0499
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Flat,Reject,_>::new(1);provider::boundary(|mut r|{let a=r.lock(&a);let _b=r.lock(provider::sync::global_commit());std::hint::black_box(&*a);});}
