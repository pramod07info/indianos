//! R025/13: dependency-ordered multi-stage build graph with trusted CI results.
//! Stages are planned and checked here; real native build execution is external.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage{Discover,Compile,Link,Test,Package,Sign}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Passed,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub parent:u32,pub stage:Stage,
 pub state:State,pub toolchain:u64,pub result:i32}
pub struct Graph<const N:usize>{owner:u32,jobs:[Option<Job>;N]}
impl<const N:usize> Graph<N>{
 pub const fn new(owner:u32)->Self{Self{owner,jobs:[None;N]}}
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.jobs.iter().position(|j|j.is_some_and(|j|j.id==id))
 }
 pub fn add(&mut self,owner:u32,id:u32,parent:u32,
  stage:Stage,toolchain:u64)->Result<(),Error>{
  self.auth(owner)?;
  if id==0||id==parent||toolchain==0||self.find(id).is_some(){
   return Err(Error::Invalid);
  }
  if parent!=0&&self.find(parent).is_none(){return Err(Error::Missing);}
  let at=self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.jobs[at]=Some(Job{id,parent,stage,toolchain,state:State::Queued,result:0});Ok(())
 }
 pub fn start(&mut self,owner:u32,id:u32,toolchain:u64)->Result<(),Error>{
  self.auth(owner)?;
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut job=self.jobs[i].ok_or(Error::Missing)?;
  if job.state!=State::Queued||job.toolchain!=toolchain{return Err(Error::Denied);}
  if job.parent!=0&&!self.find(job.parent).is_some_and(|j|
   self.jobs[j].is_some_and(|p|p.state==State::Passed)){
   return Err(Error::NotReady);
  }
  job.state=State::Running;self.jobs[i]=Some(job);Ok(())
 }
 pub fn finish(&mut self,trusted:bool,id:u32,result:i32)->Result<State,Error>{
  if !trusted{return Err(Error::Denied);}
  let i=self.find(id).ok_or(Error::Missing)?;
  let mut job=self.jobs[i].ok_or(Error::Missing)?;
  if job.state!=State::Running{return Err(Error::Conflict);}
  job.result=result;job.state=if result==0{State::Passed}else{State::Failed};
  self.jobs[i]=Some(job);Ok(job.state)
 }
 pub fn all_passed(&self,owner:u32)->Result<bool,Error>{
  self.auth(owner)?;
  Ok(self.jobs.iter().flatten().count()>0&&
   self.jobs.iter().flatten().all(|j|j.state==State::Passed))
 }
 pub fn last(&self,owner:u32,id:u32)->Result<Job,Error>{
  self.auth(owner)?;self.find(id).and_then(|i|self.jobs[i]).ok_or(Error::Missing)
 }
 pub fn count(&self,owner:u32)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.jobs.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compiler_linker_test_and_package_stages_require_prior_success(){
  let mut g=Graph::<3>::new(7);
  g.add(7,1,0,Stage::Compile,21).unwrap();
  g.add(7,2,1,Stage::Link,21).unwrap();
  g.add(7,3,2,Stage::Test,21).unwrap();
  assert_eq!(g.start(7,3,21),Err(Error::NotReady));
  for id in 1..=3{g.start(7,id,21).unwrap();g.finish(true,id,0).unwrap();}
  assert_eq!(g.all_passed(7),Ok(true));
 }
 #[test]fn trusted_failure_prevents_downstream_release_stage(){
  let mut g=Graph::<2>::new(7);
  g.add(7,1,0,Stage::Compile,21).unwrap();
  g.add(7,2,1,Stage::Package,21).unwrap();
  g.start(7,1,21).unwrap();g.finish(true,1,1).unwrap();
  assert_eq!(g.start(7,2,21),Err(Error::NotReady));
 }
 #[test]fn forged_worker_and_mismatched_toolchain_rejected(){
  let mut g=Graph::<1>::new(7);g.add(7,1,0,Stage::Compile,21).unwrap();
  assert_eq!(g.start(7,1,17),Err(Error::Denied));
  assert_eq!(g.finish(false,1,0),Err(Error::Denied));
 }
}
