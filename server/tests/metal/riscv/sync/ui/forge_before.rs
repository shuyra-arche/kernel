// expect: E0277
#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::metal;
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
impl order::Before<order::Nest> for order::Flat {} fn main(){}
