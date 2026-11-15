// expect: E0594
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::metal;
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Nest,Accept,_>::new(0);provider::boundary(|mut r|{let mut a=r.lock(&a);*a=2;});}
