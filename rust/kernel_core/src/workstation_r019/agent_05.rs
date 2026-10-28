//! R019/05: dependency-ordered developer task execution plan.
//! The scheduler chooses ready work; the actual compiler executor is external.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Passed,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Task{pub id:u32,pub depends_on:u32,pub owner:u32,
 pub state:State,pub attempts:u8}
pub struct Graph<const N:usize>{tasks:[Option<Task>;N]}
impl<const N:usize> Graph<N>{
 pub const fn new()->Self{Self{tasks:[None;N]}}
 fn index(&self,id:u32)->Option<usize>{
  self.tasks.iter().position(|x|x.is_some_and(|t|t.id==id))
 }
 pub fn add(&mut self,owner:u32,id:u32,depends_on:u32)->Result<(),Error>{
  if owner==0||id==0||id==depends_on||self.index(id).is_some(){
   return Err(Error::Invalid);
  }
  if depends_on!=0 {
   let parent=self.index(depends_on).ok_or(Error::Missing)?;
   if self.tasks[parent].is_none_or(|t|t.owner!=owner){return Err(Error::Denied);}
  }
  let slot=self.tasks.iter_mut().find(|t|t.is_none()).ok_or(Error::Full)?;
  *slot=Some(Task{id,depends_on,owner,state:State::Queued,attempts:0});Ok(())
 }
 pub fn start_ready(&mut self,owner:u32)->Result<Task,Error>{
  if owner==0{return Err(Error::Denied);}
  let candidate=self.tasks.iter().enumerate().find_map(|(i,x)|{
   let t=x.as_ref()?;
   if t.owner!=owner||t.state!=State::Queued{return None;}
   if t.depends_on!=0&&!self.index(t.depends_on)
    .is_some_and(|j|self.tasks[j].is_some_and(|d|d.state==State::Passed)){
    return None;
   }Some(i)
  }).ok_or(Error::NotReady)?;
  let mut task=self.tasks[candidate].ok_or(Error::Missing)?;
  task.attempts=task.attempts.checked_add(1).ok_or(Error::Overflow)?;
  task.state=State::Running;self.tasks[candidate]=Some(task);Ok(task)
 }
 pub fn finish(&mut self,owner:u32,id:u32,success:bool)->Result<(),Error>{
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut task=self.tasks[i].ok_or(Error::Missing)?;
  if task.owner!=owner{return Err(Error::Denied);}
  if task.state!=State::Running{return Err(Error::Conflict);}
  task.state=if success{State::Passed}else{State::Failed};
  self.tasks[i]=Some(task);Ok(())
 }
 pub fn retry(&mut self,owner:u32,id:u32,max_attempts:u8)->Result<(),Error>{
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut t=self.tasks[i].ok_or(Error::Missing)?;
  if t.owner!=owner{return Err(Error::Denied);}
  if t.state!=State::Failed||t.attempts>=max_attempts{return Err(Error::Conflict);}
  t.state=State::Queued;self.tasks[i]=Some(t);Ok(())
 }
 pub fn status(&self,owner:u32,id:u32)->Result<Task,Error>{
  let i=self.index(id).ok_or(Error::Missing)?;
  let t=self.tasks[i].ok_or(Error::Missing)?;
  if owner==0||t.owner!=owner{Err(Error::Denied)}else{Ok(t)}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compile_test_package_are_dependency_ordered(){
  let mut g=Graph::<3>::new();
  g.add(7,1,0).unwrap();g.add(7,2,1).unwrap();g.add(7,3,2).unwrap();
  assert_eq!(g.start_ready(7).unwrap().id,1);
  assert_eq!(g.start_ready(7),Err(Error::NotReady));
  g.finish(7,1,true).unwrap();assert_eq!(g.start_ready(7).unwrap().id,2);
  g.finish(7,2,true).unwrap();assert_eq!(g.start_ready(7).unwrap().id,3);
 }
 #[test]fn failed_build_blocks_dependent_test_until_retry(){
  let mut g=Graph::<2>::new();g.add(1,1,0).unwrap();g.add(1,2,1).unwrap();
  g.start_ready(1).unwrap();g.finish(1,1,false).unwrap();
  assert_eq!(g.start_ready(1),Err(Error::NotReady));
  g.retry(1,1,2).unwrap();g.start_ready(1).unwrap();g.finish(1,1,true).unwrap();
  assert_eq!(g.start_ready(1).unwrap().id,2);
 }
 #[test]fn foreign_user_cannot_finish_or_replay_task(){
  let mut g=Graph::<1>::new();g.add(1,1,0).unwrap();g.start_ready(1).unwrap();
  assert_eq!(g.finish(2,1,true),Err(Error::Denied));
  g.finish(1,1,true).unwrap();assert_eq!(g.finish(1,1,true),Err(Error::Conflict));
 }
}
