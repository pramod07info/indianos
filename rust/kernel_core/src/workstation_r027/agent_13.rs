//! R027/13: bounded DAG build dependency planner and trusted status transitions.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Pending,Running,Passed,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Step{pub id:u32,pub requires:u32,pub toolchain:u64,
 pub state:State,pub result:i32}
pub struct Plan<const N:usize>{owner:u32,steps:[Option<Step>;N]}
impl<const N:usize> Plan<N>{
 pub const fn new(owner:u32)->Self{Self{owner,steps:[None;N]}}
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.steps.iter().position(|x|x.is_some_and(|s|s.id==id))
 }
 pub fn add(&mut self,owner:u32,id:u32,parent:u32,
 toolchain:u64)->Result<(),Error>{
  self.auth(owner)?;
  if id==0||id==parent||toolchain==0||self.find(id).is_some(){
   return Err(Error::Invalid);
  }
  if parent!=0&&self.find(parent).is_none(){return Err(Error::Missing);}
  let slot=self.steps.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.steps[slot]=Some(Step{id,requires:parent,toolchain,
   state:State::Pending,result:0});Ok(())
 }
 pub fn start(&mut self,owner:u32,id:u32,toolchain:u64)->Result<(),Error>{
  self.auth(owner)?;
  let slot=self.find(id).ok_or(Error::Missing)?;
  let mut job=self.steps[slot].ok_or(Error::Missing)?;
  if job.state!=State::Pending||job.toolchain!=toolchain{return Err(Error::Denied);}
  if job.requires!=0&&!self.find(job.requires).is_some_and(|i|
   self.steps[i].is_some_and(|x|x.state==State::Passed)){
   return Err(Error::NotReady);
  }
  job.state=State::Running;self.steps[slot]=Some(job);Ok(())
 }
 pub fn complete(&mut self,trusted:bool,id:u32,exit:i32)->Result<State,Error>{
  if !trusted{return Err(Error::Denied);}
  let slot=self.find(id).ok_or(Error::Missing)?;
  let mut job=self.steps[slot].ok_or(Error::Missing)?;
  if job.state!=State::Running{return Err(Error::Conflict);}
  job.result=exit;job.state=if exit==0{State::Passed}else{State::Failed};
  self.steps[slot]=Some(job);Ok(job.state)
 }
 pub fn ready(&self,owner:u32)->Result<bool,Error>{
  self.auth(owner)?;
  Ok(self.steps.iter().flatten().count()>0&&
   self.steps.iter().flatten().all(|s|s.state==State::Passed))
 }
 pub fn count(&self,owner:u32)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.steps.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compile_must_pass_before_link_and_test_plan_runs(){
  let mut p=Plan::<2>::new(7);
  p.add(7,1,0,21).unwrap();p.add(7,2,1,21).unwrap();
  assert_eq!(p.start(7,2,21),Err(Error::NotReady));
  p.start(7,1,21).unwrap();p.complete(true,1,0).unwrap();
  p.start(7,2,21).unwrap();p.complete(true,2,0).unwrap();
  assert_eq!(p.ready(7),Ok(true));
 }
 #[test]fn failed_compile_never_unlocks_child_step(){
  let mut p=Plan::<2>::new(7);p.add(7,1,0,21).unwrap();p.add(7,2,1,21).unwrap();
  p.start(7,1,21).unwrap();p.complete(true,1,1).unwrap();
  assert_eq!(p.start(7,2,21),Err(Error::NotReady));
 }
 #[test]fn untrusted_test_worker_cannot_forge_build_success(){
  let mut p=Plan::<1>::new(7);p.add(7,1,0,21).unwrap();
  p.start(7,1,21).unwrap();
  assert_eq!(p.complete(false,1,0),Err(Error::Denied));
 }
}
