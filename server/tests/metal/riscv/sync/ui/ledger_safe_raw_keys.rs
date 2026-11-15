// expect: E0133
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::LedgerSerial,Reject,_>::new(());let b=Lock::<order::LedgerSerial,Reject,_>::new(());let _pair=provider::sync::lock::LedgerSerialPairGuard::legacy(&a,1,&b,2);}
