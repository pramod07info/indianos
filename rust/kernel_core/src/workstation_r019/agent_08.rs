//! R019/08 authenticated debugger session lifecycle and stop-event inbox.
//! Debugging a target requires a trusted supervisor to bind the owner first.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Idle,Attached,Running,Stopped,Detached}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct StopEvent{pub target:u32,pub address:u64,pub reason:u8,pub tick:u64}
pub struct Session<const N:usize>{owner:u32,target:u32,state:State,
 inbox:[Option<StopEvent>;N],len:usize,last_tick:u64}
impl<const N:usize> Session<N>{
 pub const fn new()->Self{Self{owner:0,target:0,state:State::Idle,
  inbox:[None;N],len:0,last_tick:0}}
 fn guard(&self,caller:u32)->Result<(),Error>{
  if caller==0||self.owner!=caller{return Err(Error::Denied);}Ok(())
 }
 pub fn attach(&mut self,owner:u32,target:u32,approved:bool)->Result<(),Error>{
  if owner==0||target==0||owner==target||!approved{return Err(Error::Denied);}
  if self.state!=State::Idle&&self.state!=State::Detached{return Err(Error::Conflict);}
  *self=Self::new();self.owner=owner;self.target=target;
  self.state=State::Attached;Ok(())
 }
 pub fn resume(&mut self,caller:u32)->Result<(),Error>{
  self.guard(caller)?;
  if self.state!=State::Attached&&self.state!=State::Stopped{
   return Err(Error::Conflict);
  }
  self.state=State::Running;Ok(())
 }
 pub fn report_stop(&mut self,supervisor:bool,event:StopEvent)
  ->Result<(),Error>{
  if !supervisor||event.target!=self.target||event.address==0||
   event.reason==0{return Err(Error::Denied);}
  if self.state!=State::Running||event.tick<self.last_tick{return Err(Error::Stale);}
  if self.len==N{return Err(Error::Full);}
  self.inbox[self.len]=Some(event);self.len+=1;
  self.last_tick=event.tick;self.state=State::Stopped;Ok(())
 }
 pub fn take(&mut self,caller:u32)->Result<StopEvent,Error>{
  self.guard(caller)?;
  if self.len==0{return Err(Error::Missing);}
  let e=self.inbox[0].ok_or(Error::Missing)?;
  for i in 1..self.len{self.inbox[i-1]=self.inbox[i];}
  self.len-=1;self.inbox[self.len]=None;Ok(e)
 }
 pub fn detach(&mut self,caller:u32)->Result<(),Error>{
  self.guard(caller)?;*self=Self::new();Ok(())
 }
 pub fn state(&self)->State{self.state}
}
#[cfg(test)]mod tests{
 use super::*;
 fn stop(target:u32,tick:u64)->StopEvent{
  StopEvent{target,address:0x1000,reason:3,tick}}
 #[test]fn attached_debugger_observes_supervisor_stop(){
  let mut d=Session::<2>::new();d.attach(10,20,true).unwrap();d.resume(10).unwrap();
  d.report_stop(true,stop(20,1)).unwrap();
  assert_eq!(d.state(),State::Stopped);
  assert_eq!(d.take(10),Ok(stop(20,1)));
  d.resume(10).unwrap();d.detach(10).unwrap();assert_eq!(d.state(),State::Idle);
 }
 #[test]fn foreign_debugger_and_user_generated_stop_are_denied(){
  let mut d=Session::<1>::new();
  assert!(d.attach(10,20,false).is_err());
  d.attach(10,20,true).unwrap();d.resume(10).unwrap();
  assert!(d.report_stop(false,stop(20,1)).is_err());
  assert_eq!(d.take(11),Err(Error::Denied));assert_eq!(d.state(),State::Running);
 }
 #[test]fn stale_clock_and_wrong_target_cannot_spoof_stop(){
  let mut d=Session::<1>::new();d.attach(10,20,true).unwrap();d.resume(10).unwrap();
  d.report_stop(true,stop(20,5)).unwrap();d.take(10).unwrap();d.resume(10).unwrap();
  assert_eq!(d.report_stop(true,stop(20,4)),Err(Error::Stale));
  assert_eq!(d.report_stop(true,stop(21,6)),Err(Error::Denied));
 }
}
