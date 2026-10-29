//! R031/14: bounded terminal job lifecycle with cancellation and verified exits.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Exited(i32),Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub state:State,pub deadline:u64}
pub struct Queue<const N:usize>{owner:u64,jobs:[Option<Job>;N]}
impl<const N:usize> Queue<N>{
 pub fn new(owner:u64)->Self{Self{owner,jobs:[None;N]}}
 fn allowed(&self,owner:u64)->Result<(),Error>{
  if owner==0||self.owner!=owner{Err(Error::Denied)}else{Ok(())}
 }
 pub fn enqueue(&mut self,owner:u64,id:u32,deadline:u64)->Result<(),Error>{
  self.allowed(owner)?;
  if id==0||deadline==0||self.jobs.iter().flatten().any(|j|j.id==id){
   return Err(Error::Invalid);
  }
  let slot=self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.jobs[slot]=Some(Job{id,state:State::Queued,deadline});Ok(())
 }
 pub fn start(&mut self,owner:u64,id:u32,now:u64)->Result<(),Error>{
  self.allowed(owner)?;
  let j=self.jobs.iter_mut().flatten().find(|j|j.id==id).ok_or(Error::Missing)?;
  if j.state!=State::Queued||now>=j.deadline{return Err(Error::Stale);}
  j.state=State::Running;Ok(())
 }
 pub fn finish(&mut self,owner:u64,trusted:bool,id:u32,status:i32)
  ->Result<(),Error>{
  self.allowed(owner)?;
  if !trusted{return Err(Error::Denied);}
  let j=self.jobs.iter_mut().flatten().find(|j|j.id==id).ok_or(Error::Missing)?;
  if j.state!=State::Running{return Err(Error::Stale);}
  j.state=State::Exited(status);Ok(())
 }
 pub fn cancel(&mut self,owner:u64,id:u32)->Result<(),Error>{
  self.allowed(owner)?;
  let j=self.jobs.iter_mut().flatten().find(|j|j.id==id).ok_or(Error::Missing)?;
  if matches!(j.state,State::Exited(_)|State::Cancelled){return Err(Error::Stale);}
  j.state=State::Cancelled;Ok(())
 }
 pub fn state(&self,owner:u64,id:u32)->Result<State,Error>{
  self.allowed(owner)?;
  self.jobs.iter().flatten().find(|j|j.id==id).map(|j|j.state).ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_job_enqueued_started_and_finished_by_trusted_worker(){
  let mut jobs=Queue::<1>::new(7);jobs.enqueue(7,1,100).unwrap();
  jobs.start(7,1,10).unwrap();jobs.finish(7,true,1,0).unwrap();
  assert_eq!(jobs.state(7,1),Ok(State::Exited(0)));
 }
 #[test]fn cancelling_running_job_prevents_late_stale_completion(){
  let mut jobs=Queue::<1>::new(7);jobs.enqueue(7,1,100).unwrap();
  jobs.start(7,1,10).unwrap();jobs.cancel(7,1).unwrap();
  assert_eq!(jobs.finish(7,true,1,0),Err(Error::Stale));
  assert_eq!(jobs.state(8,1),Err(Error::Denied));
 }
}