// expect: E0277
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::metal;
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn sync<T:Sync>(_:&T){} fn main(){let a=Lock::<order::Flat,Reject,_>::new(1);provider::boundary(|mut r|sync(&r.lock(&a)));}
