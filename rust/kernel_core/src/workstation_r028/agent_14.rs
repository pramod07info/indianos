//! R028/14: dependency-aware build-stage scheduling policy for trusted executors.
//! This does not create external compiler processes or autonomous developer agents.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Pending,Active,Passed,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Task{pub id:u32,pub parent:u32,pub state:State}
pub struct Scheduler<const N:usize>{tasks:[Option<Task>;N],max_running:usize}
impl<const N:usize> Scheduler<N>{
 pub const fn new(max_running:usize)->Self{Self{tasks:[None;N],max_running}}
 fn slot(&self,id:u32)->Option<usize>{
  self.tasks.iter().position(|x|x.is_some_and(|t|t.id==id))
 }
 pub fn add(&mut self,id:u32,parent:u32)->Result<(),Error>{
  if id==0||id==parent||self.slot(id).is_some(){return Err(Error::Invalid);}
  if parent!=0&&self.slot(parent).is_none(){return Err(Error::Missing);}
  let i=self.tasks.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.tasks[i]=Some(Task{id,parent,state:State::Pending});Ok(())
 }
 pub fn dispatch(&mut self)->Result<Option<u32>,Error>{
  if self.max_running==0{return Err(Error::Invalid);}
  if self.tasks.iter().flatten().filter(|x|x.state==State::Active).count()
   >=self.max_running{return Ok(None);}
  for i in 0..N{
   let Some(task)=self.tasks[i]else{continue};
   if task.state!=State::Pending{continue;}
   let ready=task.parent==0||self.slot(task.parent).is_some_and(|j|
    self.tasks[j].is_some_and(|t|t.state==State::Passed));
   if ready{
    self.tasks[i]=Some(Task{state:State::Active,..task});return Ok(Some(task.id));
   }
  }Ok(None)
 }
 pub fn finish(&mut self,trusted:bool,id:u32,success:bool)->Result<(),Error>{
  if !trusted{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let t=self.tasks[i].ok_or(Error::Missing)?;
  if t.state!=State::Active{return Err(Error::Conflict);}
  self.tasks[i]=Some(Task{state:if success{State::Passed}else{State::Failed},..t});Ok(())
 }
 pub fn all_passed(&self)->bool{
  self.tasks.iter().flatten().count()>0&&
   self.tasks.iter().flatten().all(|t|t.state==State::Passed)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn two_build_dependents_start_after_parent_passes(){
  let mut s=Scheduler::<3>::new(2);
  s.add(1,0).unwrap();s.add(2,1).unwrap();s.add(3,1).unwrap();
  assert_eq!(s.dispatch(),Ok(Some(1)));assert_eq!(s.dispatch(),Ok(None));
  s.finish(true,1,true).unwrap();
  assert_eq!(s.dispatch(),Ok(Some(2)));assert_eq!(s.dispatch(),Ok(Some(3)));
  s.finish(true,2,true).unwrap();s.finish(true,3,true).unwrap();
  assert!(s.all_passed());
 }
 #[test]fn failed_parent_never_unblocks_downstream_builds(){
  let mut s=Scheduler::<2>::new(1);s.add(1,0).unwrap();s.add(2,1).unwrap();
  s.dispatch().unwrap();s.finish(true,1,false).unwrap();
  assert_eq!(s.dispatch(),Ok(None));
 }
}