// expect: E0499
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Nest,Reject,_>::new(());let b=Lock::<order::Flat,Reject,_>::new(());provider::boundary(|mut r|{let mut a=r.lock(&a);let child=a.lock(&b);let _c=a.lock(&b);std::hint::black_box(&*child);});}
