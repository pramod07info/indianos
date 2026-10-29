//! R026/12: trusted job scheduler with owner-generation isolation and cancellation.
//! This manages job state, NOT a shell process executor or CPU dispatcher.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Succeeded,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub owner:Identity,pub command:u32,pub created:u64,
 pub deadline:u64,pub result:i32,pub state:State}
pub struct Jobs<const N:usize>{owner:Identity,jobs:[Option<Job>;N]}
impl<const N:usize> Jobs<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,jobs:[None;N]}}
 fn check(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.jobs.iter().position(|j|j.is_some_and(|x|x.id==id))
 }
 pub fn enqueue(&mut self,actor:Identity,id:u32,command:u32,
  now:u64,ttl:u64)->Result<(),Error>{
  self.check(actor)?;
  if id==0||command==0||ttl==0||self.slot(id).is_some(){
   return Err(Error::Invalid);
  }
  let deadline=now.checked_add(ttl).ok_or(Error::Overflow)?;
  let at=self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.jobs[at]=Some(Job{id,owner:actor,command,created:now,
   deadline,result:0,state:State::Queued});Ok(())
 }
 pub fn start(&mut self,actor:Identity,id:u32,now:u64)->Result<(),Error>{
  self.check(actor)?;
  let at=self.slot(id).ok_or(Error::Missing)?;
  let mut job=self.jobs[at].ok_or(Error::Missing)?;
  if job.state!=State::Queued||now>=job.deadline{return Err(Error::NotReady);}
  job.state=State::Running;self.jobs[at]=Some(job);Ok(())
 }
 pub fn complete(&mut self,trusted:bool,id:u32,status:i32,
  now:u64)->Result<State,Error>{
  if !trusted{return Err(Error::Denied);}
  let at=self.slot(id).ok_or(Error::Missing)?;
  let mut job=self.jobs[at].ok_or(Error::Missing)?;
  if job.state!=State::Running||now>=job.deadline{return Err(Error::NotReady);}
  job.state=if status==0{State::Succeeded}else{State::Failed};
  job.result=status;self.jobs[at]=Some(job);Ok(job.state)
 }
 pub fn cancel(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.check(actor)?;let at=self.slot(id).ok_or(Error::Missing)?;
  let mut j=self.jobs[at].ok_or(Error::Missing)?;
  if !matches!(j.state,State::Queued|State::Running){return Err(Error::Conflict);}
  j.state=State::Cancelled;self.jobs[at]=Some(j);Ok(())
 }
 pub fn get(&self,actor:Identity,id:u32)->Result<Job,Error>{
  self.check(actor)?;
  self.slot(id).and_then(|i|self.jobs[i]).ok_or(Error::Missing)
 }
 pub fn pending(&self,actor:Identity)->Result<usize,Error>{
  self.check(actor)?;
  Ok(self.jobs.iter().flatten().filter(|j|
   matches!(j.state,State::Queued|State::Running)).count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn user_job_lifecycle_tracks_actual_trusted_execution_result(){
  let mut q=Jobs::<1>::new(id(1));q.enqueue(id(1),1,20,1,10).unwrap();
  q.start(id(1),1,2).unwrap();
  assert_eq!(q.complete(true,1,0,3),Ok(State::Succeeded));
  assert_eq!(q.pending(id(1)),Ok(0));
 }
 #[test]fn cancelled_job_cannot_be_marked_as_success(){
  let mut q=Jobs::<1>::new(id(1));q.enqueue(id(1),1,20,1,10).unwrap();
  q.cancel(id(1),1).unwrap();
  assert_eq!(q.complete(true,1,0,3),Err(Error::NotReady));
 }
 #[test]fn recycled_pid_and_untrusted_worker_cannot_manage_job(){
  let mut q=Jobs::<1>::new(id(1));q.enqueue(id(1),1,20,1,10).unwrap();
  assert_eq!(q.start(id(2),1,2),Err(Error::Denied));
  q.start(id(1),1,2).unwrap();
  assert_eq!(q.complete(false,1,0,3),Err(Error::Denied));
 }
}
