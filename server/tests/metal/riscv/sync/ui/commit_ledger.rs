// expect: E0277
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::metal;
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let b=Lock::<order::LedgerSerial,Reject,_>::new(());provider::boundary(|mut r|{let mut a=r.lock(provider::sync::global_commit());let _b=a.lock(&b);});}
