#![allow(dead_code,unused_imports,unused_variables,unused_mut)]
#[path="../provider/mod.rs"] mod provider;
#[path="../support.rs"] mod support;
pub(crate) use support::{metal, basis};
use provider::sync::{lock::{Lock,reentry::{Accept,Reject,Policy}},order,Root};
fn main(){
 let a=Lock::<order::LedgerSerial,Reject,_>::new(());
 let flat=Lock::<order::Flat,Reject,_>::new(1);
 let nest=Lock::<order::Nest,Reject,_>::new(());
 provider::boundary(|mut root|{
   let mut ledger=root.lock(&a);
   let mut commit=ledger.lock(provider::sync::global_commit());
   {let inner=commit.reenter();std::hint::black_box(&*inner);}
   {let mut state=commit.lock(&flat);*state+=1;}
   {let mut state=commit.lock(&nest);let mut child=state.try_lock(&flat).unwrap();*child+=1;}
 });
 let other=Lock::<order::Flat,Reject,_>::new(2);
 provider::boundary(|mut root|{let mut pair=root.pair(&flat,&other);let(a,b)=pair.values();*a+=*b;});
}
