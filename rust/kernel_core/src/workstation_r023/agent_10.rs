//! R023/10: build/test task retry scheduling, bounded backoff and cancellation.
//! The kernel executor must call start/finish; no arbitrary host command runs.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Status{Queued,Running,RetryWait,Passed,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Task{pub owner:u32,pub id:u32,pub dependency:u32,
 pub attempts:u8,pub max_attempts:u8,pub next_tick:u64,
 pub deadline:u64,pub status:Status,pub last_code:u16}
pub struct Scheduler<const N:usize>{owner:u32,tasks:[Option<Task>;N]}
impl<const N:usize> Scheduler<N>{
 pub const fn new(owner:u32)->Self{Self{owner,tasks:[None;N]}}
 fn auth(&self,caller:u32)->Result<(),Error>{
  if caller==0||caller!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.tasks.iter().position(|x|x.is_some_and(|t|t.id==id))
 }
 pub fn add(&mut self,owner:u32,id:u32,dependency:u32,
  attempts:u8,deadline:u64)->Result<(),Error>{
  self.auth(owner)?;
  if id==0||id==dependency||attempts==0||deadline==0||
   self.find(id).is_some(){return Err(Error::Invalid);}
  if dependency!=0&&self.find(dependency).is_none(){return Err(Error::Missing);}
  let slot=self.tasks.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.tasks[slot]=Some(Task{owner,id,dependency,attempts:0,
   max_attempts:attempts,next_tick:0,deadline,status:Status::Queued,last_code:0});
  Ok(())
 }
 pub fn start(&mut self,owner:u32,id:u32,tick:u64)->Result<(),Error>{
  self.auth(owner)?;
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut t=self.tasks[i].ok_or(Error::Missing)?;
  if !matches!(t.status,Status::Queued|Status::RetryWait)||
   tick<t.next_tick||tick>=t.deadline{return Err(Error::Stale);}
  if t.dependency!=0&&!self.find(t.dependency).is_some_and(|x|
    self.tasks[x].is_some_and(|p|p.status==Status::Passed)){
   return Err(Error::NotReady);
  }
  t.attempts=t.attempts.checked_add(1).ok_or(Error::Overflow)?;
  t.status=Status::Running;self.tasks[i]=Some(t);Ok(())
 }
 pub fn finish(&mut self,trusted:bool,id:u32,code:u16,
  tick:u64)->Result<Status,Error>{
  if !trusted{return Err(Error::Denied);}
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut t=self.tasks[i].ok_or(Error::Missing)?;
  if t.status!=Status::Running{return Err(Error::Conflict);}
  t.last_code=code;
  t.status=if code==0{Status::Passed}else if t.attempts>=t.max_attempts{
   Status::Failed
  }else{Status::RetryWait};
  if t.status==Status::RetryWait{
   let delay=1u64.checked_shl(t.attempts.min(10) as u32).ok_or(Error::Overflow)?;
   t.next_tick=tick.checked_add(delay).ok_or(Error::Overflow)?;
  }
  self.tasks[i]=Some(t);Ok(t.status)
 }
 pub fn cancel(&mut self,owner:u32,id:u32)->Result<(),Error>{
  self.auth(owner)?;
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut t=self.tasks[i].ok_or(Error::Missing)?;
  if t.status==Status::Passed{return Err(Error::Conflict);}
  t.status=Status::Cancelled;self.tasks[i]=Some(t);Ok(())
 }
 pub fn get(&self,owner:u32,id:u32)->Result<Task,Error>{
  self.auth(owner)?;
  self.find(id).and_then(|i|self.tasks[i]).ok_or(Error::Missing)
 }
 pub fn failed_count(&self,owner:u32)->Result<usize,Error>{
  self.auth(owner)?;
  Ok(self.tasks.iter().flatten().filter(|x|x.status==Status::Failed).count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compiler_failure_retries_with_backoff_then_passes(){
  let mut s=Scheduler::<2>::new(7);
  s.add(7,1,0,3,100).unwrap();s.add(7,2,1,1,100).unwrap();
  s.start(7,1,1).unwrap();
  assert_eq!(s.finish(true,1,12,2),Ok(Status::RetryWait));
  assert_eq!(s.start(7,1,3),Err(Error::Stale));
  s.start(7,1,4).unwrap();s.finish(true,1,0,5).unwrap();
  s.start(7,2,6).unwrap();s.finish(true,2,0,6).unwrap();
  assert_eq!(s.failed_count(7),Ok(0));
 }
 #[test]fn retry_budget_exhaustion_is_terminal_failure(){
  let mut s=Scheduler::<1>::new(7);s.add(7,1,0,1,100).unwrap();
  s.start(7,1,1).unwrap();
  assert_eq!(s.finish(true,1,10,2),Ok(Status::Failed));
  assert_eq!(s.start(7,1,4),Err(Error::Stale));
 }
 #[test]fn forged_executor_cannot_advance_or_reset_build_state(){
  let mut s=Scheduler::<1>::new(7);s.add(7,1,0,2,100).unwrap();
  s.start(7,1,1).unwrap();
  assert_eq!(s.finish(false,1,0,2),Err(Error::Denied));
  assert_eq!(s.get(7,1).unwrap().status,Status::Running);
 }
}
