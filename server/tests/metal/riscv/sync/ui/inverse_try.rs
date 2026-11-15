// expect: E0277
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Flat,Reject,_>::new(());let b=Lock::<order::Nest,Reject,_>::new(());provider::boundary(|mut r|{let mut a=r.lock(&a);let _b=a.try_lock(&b);});}
