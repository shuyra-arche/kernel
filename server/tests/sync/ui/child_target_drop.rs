// expect: E0597
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::metal;
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Nest,Reject,_>::new(());provider::boundary(|mut r|{let mut a=r.lock(&a);let b;{let child=Lock::<order::Flat,Reject,_>::new(1);b=a.lock(&child);}std::hint::black_box(&*b);});}
