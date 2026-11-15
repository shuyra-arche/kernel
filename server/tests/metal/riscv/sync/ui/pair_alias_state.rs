// expect: E0499
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){let a=Lock::<order::Flat,Reject,_>::new(1);let b=Lock::<order::Flat,Reject,_>::new(2);provider::boundary(|mut r|{let mut pair=r.pair(&a,&b);let (a,b)=pair.values();let _other=pair.values();*a+=*b;});}
