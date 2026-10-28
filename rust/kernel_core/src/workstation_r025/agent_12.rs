//! R025/12: generation-scoped terminal audit journal with safe command categories.
//! Logs trusted operation results, not arbitrary command strings or secret payloads.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Read,Write,Build,Test,Denied}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Event{pub owner:Identity,pub job:u32,pub seq:u64,pub tick:u64,
 pub kind:Kind,pub status:i32}
pub const EMPTY:Event=Event{owner:Identity{pid:0,generation:0},job:0,
 seq:0,tick:0,kind:Kind::Denied,status:0};
pub struct Audit<const N:usize>{owner:Identity,events:[Option<Event>;N],
 head:usize,seq:u64,last_tick:u64}
impl<const N:usize> Audit<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,events:[None;N],head:0,seq:0,last_tick:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn record(&mut self,actor:Identity,trusted:bool,job:u32,
  tick:u64,kind:Kind,status:i32)->Result<u64,Error>{
  self.auth(actor)?;
  if !trusted||job==0{return Err(Error::Denied);}
  if N==0||tick<self.last_tick{return Err(Error::Invalid);}
  let next=self.seq.checked_add(1).ok_or(Error::Overflow)?;
  self.events[self.head]=Some(Event{owner:actor,job,seq:next,tick,kind,status});
  self.head=(self.head+1)%N;self.last_tick=tick;self.seq=next;Ok(next)
 }
 pub fn since(&self,actor:Identity,after:u64,
  out:&mut[Event])->Result<usize,Error>{
  self.auth(actor)?;
  let count=self.events.iter().flatten().filter(|e|e.seq>after).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;for e in self.events.iter().flatten().filter(|e|e.seq>after){
   out[n]=*e;n+=1;
  }
  out[..n].sort_unstable_by_key(|e|e.seq);Ok(n)
 }
 pub fn failures(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;
  Ok(self.events.iter().flatten().filter(|e|e.status!=0).count())
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  self.auth(actor)?;self.events=[None;N];self.head=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn trusted_terminal_commands_emit_ordered_audit_events(){
  let mut a=Audit::<2>::new(id(1));
  a.record(id(1),true,1,1,Kind::Build,0).unwrap();
  a.record(id(1),true,2,2,Kind::Test,10).unwrap();
  let mut out=[EMPTY;2];assert_eq!(a.since(id(1),0,&mut out),Ok(2));
  assert_eq!(a.failures(id(1)),Ok(1));
 }
 #[test]fn untrusted_worker_cannot_forge_successful_commands(){
  let mut a=Audit::<1>::new(id(1));
  assert_eq!(a.record(id(1),false,1,1,Kind::Build,0),Err(Error::Denied));
 }
 #[test]fn foreign_generation_cannot_read_terminal_history(){
  let a=Audit::<1>::new(id(1));let mut out=[EMPTY;1];
  assert_eq!(a.since(id(2),0,&mut out),Err(Error::Denied));
 }
}
