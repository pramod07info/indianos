//! R022/16: terminal foreground/background task lifecycle and exit capture.
//! State and supervision only; does NOT create ring-3 processes itself.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Stopped,Exited,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub owner:Identity,pub pid:u32,
 pub created:u64,pub ended:u64,pub exit:i32,pub state:State,
 pub foreground:bool}
pub struct Terminal<const N:usize>{owner:Identity,
 jobs:[Option<Job>;N],foreground:u32}
impl<const N:usize> Terminal<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,jobs:[None;N],foreground:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn index(&self,id:u32)->Option<usize>{
  self.jobs.iter().position(|j|j.is_some_and(|j|j.id==id))
 }
 pub fn enqueue(&mut self,actor:Identity,id:u32,
  tick:u64,foreground:bool)->Result<(),Error>{
  self.auth(actor)?;
  if id==0||self.index(id).is_some(){return Err(Error::Invalid);}
  if foreground&&self.foreground!=0{return Err(Error::Conflict);}
  let at=self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.jobs[at]=Some(Job{id,owner:actor,pid:0,created:tick,ended:0,
   exit:0,state:State::Queued,foreground});
  if foreground{self.foreground=id;}Ok(())
 }
 pub fn started(&mut self,supervisor:bool,id:u32,pid:u32)
  ->Result<(),Error>{
  if !supervisor||pid==0{return Err(Error::Denied);}
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut j=self.jobs[i].ok_or(Error::Missing)?;
  if j.state!=State::Queued{return Err(Error::Conflict);}
  j.pid=pid;j.state=State::Running;self.jobs[i]=Some(j);Ok(())
 }
 pub fn stop(&mut self,supervisor:bool,id:u32)->Result<(),Error>{
  if !supervisor{return Err(Error::Denied);}
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut j=self.jobs[i].ok_or(Error::Missing)?;
  if j.state!=State::Running{return Err(Error::Conflict);}
  j.state=State::Stopped;self.jobs[i]=Some(j);Ok(())
 }
 pub fn resume(&mut self,supervisor:bool,id:u32)->Result<(),Error>{
  if !supervisor{return Err(Error::Denied);}
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut j=self.jobs[i].ok_or(Error::Missing)?;
  if j.state!=State::Stopped{return Err(Error::Conflict);}
  j.state=State::Running;self.jobs[i]=Some(j);Ok(())
 }
 pub fn exited(&mut self,supervisor:bool,id:u32,
  exit:i32,tick:u64)->Result<(),Error>{
  if !supervisor{return Err(Error::Denied);}
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut j=self.jobs[i].ok_or(Error::Missing)?;
  if !matches!(j.state,State::Running|State::Stopped)||tick<j.created{
   return Err(Error::Conflict);
  }
  j.state=if exit==0{State::Exited}else{State::Failed};
  j.exit=exit;j.ended=tick;self.jobs[i]=Some(j);
  if self.foreground==id{self.foreground=0;}
  Ok(())
 }
 pub fn get(&self,actor:Identity,id:u32)->Result<Job,Error>{
  self.auth(actor)?;
  self.index(id).and_then(|i|self.jobs[i]).ok_or(Error::Missing)
 }
 pub fn foreground(&self,actor:Identity)->Result<u32,Error>{
  self.auth(actor)?;Ok(self.foreground)
 }
 pub fn reap(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.auth(actor)?;
  let i=self.index(id).ok_or(Error::Missing)?;
  let j=self.jobs[i].ok_or(Error::Missing)?;
  if !matches!(j.state,State::Exited|State::Failed|State::Cancelled){
   return Err(Error::Conflict);
  }
  self.jobs[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn terminal_foreground_build_job_reports_real_exit_status(){
  let mut t=Terminal::<2>::new(id(1));
  t.enqueue(id(1),1,1,true).unwrap();t.started(true,1,101).unwrap();
  assert_eq!(t.foreground(id(1)),Ok(1));
  t.exited(true,1,0,2).unwrap();assert_eq!(t.foreground(id(1)),Ok(0));
  assert_eq!(t.get(id(1),1).unwrap().state,State::Exited);
  t.reap(id(1),1).unwrap();
 }
 #[test]fn foreign_pid_and_forged_process_exit_are_denied(){
  let mut t=Terminal::<1>::new(id(1));t.enqueue(id(1),1,1,true).unwrap();
  assert_eq!(t.started(false,1,101),Err(Error::Denied));
  assert_eq!(t.get(id(2),1),Err(Error::Denied));
 }
 #[test]fn second_foreground_task_must_wait_for_first_to_exit(){
  let mut t=Terminal::<2>::new(id(1));t.enqueue(id(1),1,1,true).unwrap();
  assert_eq!(t.enqueue(id(1),2,2,true),Err(Error::Conflict));
  assert_eq!(t.foreground(id(1)),Ok(1));
 }
}
