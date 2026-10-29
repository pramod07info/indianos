//! R032/14: build graph with required stage dependencies and failure propagation.
use super::Error;
use crate::workstation_r030::agent_15;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Status{Pending,Running,Passed,Failed,Skipped}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Step{pub id:u8,pub depends:u32,pub status:Status}
pub struct Plan<const N:usize>{owner:u64,steps:[Option<Step>;N]}
impl<const N:usize> Plan<N>{
 pub fn new(owner:u64)->Self{Self{owner,steps:[None;N]}}
 pub fn add(&mut self,owner:u64,id:u8,depends:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if id>=32||depends&(1u32<<id)!=0||
   self.steps.iter().flatten().any(|s|s.id==id){return Err(Error::Invalid);}
  let i=self.steps.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.steps[i]=Some(Step{id,depends,status:Status::Pending});Ok(())
 }
 pub fn order(&self,owner:u64,out:&mut[u8])->Result<usize,Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  let mut tasks=[agent_15::Job{id:0,dependencies:0};N];let mut n=0;
  for s in self.steps.iter().flatten(){
   tasks[n]=agent_15::Job{id:s.id,dependencies:s.depends};n+=1;
  }
  agent_15::order(&tasks[..n],out).map_err(|_|Error::Conflict)
 }
 pub fn next(&mut self,owner:u64)->Result<Option<u8>,Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  let mut completed=0u32;let mut failed=0u32;
  for s in self.steps.iter().flatten(){
   if s.status==Status::Passed{completed|=1u32<<s.id;}
   if s.status==Status::Failed||s.status==Status::Skipped{failed|=1u32<<s.id;}
   if s.status==Status::Running{return Err(Error::NotReady);}
  }
  for s in self.steps.iter_mut().flatten(){
   if s.status==Status::Pending&&s.depends&failed!=0{s.status=Status::Skipped;}
  }
  let mut candidate=None;
  for s in self.steps.iter().flatten(){
   if s.status==Status::Pending&&s.depends&!completed==0&&
    candidate.is_none_or(|id:u8|s.id<id){candidate=Some(s.id);}
  }
  if let Some(id)=candidate{
   let s=self.steps.iter_mut().flatten().find(|s|s.id==id).ok_or(Error::Missing)?;
   s.status=Status::Running;return Ok(Some(id));
  }Ok(None)
 }
 pub fn finish(&mut self,owner:u64,trusted:bool,id:u8,passed:bool)->Result<(),Error>{
  if owner!=self.owner||owner==0||!trusted{return Err(Error::Denied);}
  let s=self.steps.iter_mut().flatten().find(|s|s.id==id).ok_or(Error::Missing)?;
  if s.status!=Status::Running{return Err(Error::Stale);}
  s.status=if passed{Status::Passed}else{Status::Failed};Ok(())
 }
 pub fn green(&self)->bool{
  self.steps.iter().flatten().next().is_some()&&
  self.steps.iter().flatten().all(|s|s.status==Status::Passed)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compile_then_test_then_package_have_verified_order(){
  let mut plan=Plan::<3>::new(7);
  plan.add(7,2,1<<1).unwrap();plan.add(7,1,1<<0).unwrap();plan.add(7,0,0).unwrap();
  let mut order=[0;3];assert_eq!(plan.order(7,&mut order),Ok(3));
  assert_eq!(order,[0,1,2]);
  for id in 0..3{assert_eq!(plan.next(7),Ok(Some(id)));plan.finish(7,true,id,true).unwrap();}
  assert!(plan.green());
 }
 #[test]fn failed_build_blocks_dependent_packaging_and foreign_completion(){
  let mut plan=Plan::<2>::new(7);plan.add(7,0,0).unwrap();plan.add(7,1,1).unwrap();
  assert_eq!(plan.next(7),Ok(Some(0)));
  assert_eq!(plan.finish(8,true,0,true),Err(Error::Denied));
  plan.finish(7,true,0,false).unwrap();
  assert_eq!(plan.next(7),Ok(None));assert!(!plan.green());
 }
}