// expect: E0277
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn send<T:Send>(_:T){} fn main(){let a=Lock::<order::Flat,Reject,_>::new(1);provider::boundary(|mut r|send(r.lock(&a)));}
