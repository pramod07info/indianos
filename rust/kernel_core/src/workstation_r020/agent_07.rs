//! R020/07 build pipeline transaction, deadlines, stage results and failures.
//! The scheduler is a state machine; no host commands execute here.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Passed,Failed,Cancelled}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Stage{pub owner:u32,pub id:u32,pub parent:u32,pub state:State,
 pub deadline:u64,pub result:u16}
pub struct Pipeline<const N:usize>{owner:u32,stages:[Option<Stage>;N]}
impl<const N:usize> Pipeline<N>{
 pub const fn new(owner:u32)->Self{Self{owner,stages:[None;N]}}
 fn check(&self,caller:u32)->Result<(),Error>{
  if caller==0||caller!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn index(&self,id:u32)->Option<usize>{
  self.stages.iter().position(|s|s.is_some_and(|s|s.id==id))
 }
 pub fn add(&mut self,owner:u32,id:u32,parent:u32,deadline:u64)
  ->Result<(),Error>{
  self.check(owner)?;
  if id==0||deadline==0||parent==id||self.index(id).is_some(){
   return Err(Error::Invalid);
  }
  if parent!=0&&self.index(parent).is_none(){return Err(Error::Missing);}
  let free=self.stages.iter_mut().find(|s|s.is_none()).ok_or(Error::Full)?;
  *free=Some(Stage{owner,id,parent,state:State::Queued,deadline,result:0});Ok(())
 }
 pub fn start(&mut self,owner:u32,id:u32,now:u64)->Result<(),Error>{
  self.check(owner)?;
  let i=self.index(id).ok_or(Error::Missing)?;
  let t=self.stages[i].ok_or(Error::Missing)?;
  if t.state!=State::Queued||now>=t.deadline{return Err(Error::Stale);}
  if t.parent!=0&&!self.index(t.parent).is_some_and(|p|
    self.stages[p].is_some_and(|p|p.state==State::Passed)){
   return Err(Error::NotReady);
  }
  let mut t=t;t.state=State::Running;self.stages[i]=Some(t);Ok(())
 }
 pub fn finish(&mut self,owner:u32,id:u32,success:bool,code:u16)
  ->Result<(),Error>{
  self.check(owner)?;
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut t=self.stages[i].ok_or(Error::Missing)?;
  if t.state!=State::Running||(!success&&code==0){return Err(Error::Conflict);}
  t.state=if success{State::Passed}else{State::Failed};
  t.result=code;self.stages[i]=Some(t);Ok(())
 }
 pub fn cancel_dependents(&mut self,owner:u32,id:u32)
  ->Result<usize,Error>{
  self.check(owner)?;
  let parent=self.index(id).ok_or(Error::Missing)?;
  let node=self.stages[parent].ok_or(Error::Missing)?;
  if node.state!=State::Failed&&node.state!=State::Cancelled{
   return Err(Error::Conflict);
  }
  let mut count=0;
  for _ in 0..N{
   let mut updated=false;
   for i in 0..N{
    let Some(mut stage)=self.stages[i]else{continue};
    if stage.state!=State::Queued||stage.parent==0{continue;}
    if self.index(stage.parent).is_some_and(|j|self.stages[j].is_some_and(
      |p|p.state==State::Failed||p.state==State::Cancelled)){
      stage.state=State::Cancelled;self.stages[i]=Some(stage);
      count+=1;updated=true;
    }
   }
   if !updated{break;}
  }Ok(count)
 }
 pub fn status(&self,owner:u32,id:u32)->Result<Stage,Error>{
  self.check(owner)?;self.index(id).and_then(|i|self.stages[i]).ok_or(Error::Missing)
 }
 pub fn successful(&self,owner:u32)->Result<usize,Error>{
  self.check(owner)?;
  Ok(self.stages.iter().flatten().filter(|s|s.state==State::Passed).count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compile_then_test_then_package_with_deadline_and_success(){
  let mut p=Pipeline::<3>::new(7);
  p.add(7,1,0,100).unwrap();p.add(7,2,1,100).unwrap();
  p.add(7,3,2,100).unwrap();
  assert_eq!(p.start(7,2,1),Err(Error::NotReady));
  for stage in [1,2,3]{p.start(7,stage,1).unwrap();p.finish(7,stage,true,0).unwrap();}
  assert_eq!(p.successful(7),Ok(3));
 }
 #[test]fn failed_compile_cancels_all_unstarted_descendants(){
  let mut p=Pipeline::<3>::new(7);
  p.add(7,1,0,100).unwrap();p.add(7,2,1,100).unwrap();
  p.add(7,3,2,100).unwrap();p.start(7,1,1).unwrap();
  p.finish(7,1,false,12).unwrap();
  assert_eq!(p.cancel_dependents(7,1),Ok(2));
  assert_eq!(p.status(7,3).unwrap().state,State::Cancelled);
 }
 #[test]fn expired_or_foreign_job_cannot_start(){
  let mut p=Pipeline::<1>::new(7);p.add(7,1,0,2).unwrap();
  assert_eq!(p.start(7,1,2),Err(Error::Stale));
  assert_eq!(p.start(8,1,1),Err(Error::Denied));
 }
}
