//! R028/12: owned bounded terminal command jobs, trusted completion.
use super::{Error,agent_10};
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Pending,Running,Passed,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub kind:agent_10::Command,pub state:State,pub deadline:u64}
pub struct Queue<const N:usize>{owner:Identity,items:[Option<Job>;N]}
impl<const N:usize> Queue<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.items.iter().position(|x|x.is_some_and(|j|j.id==id))
 }
 pub fn submit(&mut self,who:Identity,id:u32,kind:agent_10::Command,
 now:u64,ttl:u64)->Result<(),Error>{
  self.auth(who)?;
  if id==0||ttl==0||self.slot(id).is_some(){return Err(Error::Invalid);}
  let deadline=now.checked_add(ttl).ok_or(Error::Overflow)?;
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[i]=Some(Job{id,kind,state:State::Pending,deadline});Ok(())
 }
 pub fn start(&mut self,who:Identity,id:u32,now:u64)->Result<(),Error>{
  self.auth(who)?;let i=self.slot(id).ok_or(Error::Missing)?;
  let mut job=self.items[i].ok_or(Error::Missing)?;
  if job.state!=State::Pending||now>=job.deadline{return Err(Error::NotReady);}
  job.state=State::Running;self.items[i]=Some(job);Ok(())
 }
 pub fn finish(&mut self,trusted:bool,id:u32,exit:i32)->Result<State,Error>{
  if !trusted{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut job=self.items[i].ok_or(Error::Missing)?;
  if job.state!=State::Running{return Err(Error::NotReady);}
  job.state=if exit==0{State::Passed}else{State::Failed};
  self.items[i]=Some(job);Ok(job.state)
 }
 pub fn cancel(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let i=self.slot(id).ok_or(Error::Missing)?;
  let mut j=self.items[i].ok_or(Error::Missing)?;
  if !matches!(j.state,State::Pending|State::Running){return Err(Error::Conflict);}
  j.state=State::Cancelled;self.items[i]=Some(j);Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(n:u64)->Identity{Identity{pid:7,generation:n}}
 #[test]fn real_terminal_job_state_requires_trusted_worker(){
  let mut q=Queue::<1>::new(id(1));q.submit(id(1),1,agent_10::Command::Cat,1,10).unwrap();
  q.start(id(1),1,2).unwrap();assert_eq!(q.finish(true,1,0),Ok(State::Passed));
 }
 #[test]fn recycled_session_and_cancelled_job_block_execution(){
  let mut q=Queue::<1>::new(id(1));q.submit(id(1),1,agent_10::Command::Cat,1,10).unwrap();
  assert_eq!(q.start(id(2),1,2),Err(Error::Denied));
  q.cancel(id(1),1).unwrap();assert_eq!(q.finish(true,1,0),Err(Error::NotReady));
 }
}