//! R021/06 resource-aware compile/test job reservations and accounting.
//! Scheduler metadata only; trusted kernel worker must enforce real limits.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum State{Queued,Running,Done,Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Job{pub id:u32,pub owner:u32,pub ram_mb:u32,pub cpu_slots:u16,
 pub state:State,pub deadline:u64}
pub struct Pool<const N:usize>{ram:u32,cpus:u16,jobs:[Option<Job>;N]}
impl<const N:usize> Pool<N>{
 pub const fn new(ram:u32,cpus:u16)->Self{
  Self{ram,cpus,jobs:[None;N]}
 }
 pub fn submit(&mut self,id:u32,owner:u32,ram_mb:u32,
  cpu_slots:u16,deadline:u64)->Result<(),Error>{
  if id==0||owner==0||ram_mb==0||cpu_slots==0||deadline==0||
   ram_mb>self.ram||cpu_slots>self.cpus{return Err(Error::Invalid);}
  if self.jobs.iter().flatten().any(|j|j.id==id){return Err(Error::Conflict);}
  let at=self.jobs.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.jobs[at]=Some(Job{id,owner,ram_mb,cpu_slots,deadline,state:State::Queued});
  Ok(())
 }
 pub fn usage(&self)->(u32,u32){
  self.jobs.iter().flatten().filter(|j|j.state==State::Running)
   .fold((0,0),|(m,c),j|(m+j.ram_mb,c+j.cpu_slots as u32))
 }
 pub fn start(&mut self,owner:u32,id:u32,now:u64)->Result<(),Error>{
  if owner==0{return Err(Error::Denied);}
  let at=self.jobs.iter().position(|j|j.is_some_and(|j|j.id==id&&j.owner==owner))
   .ok_or(Error::Missing)?;
  let mut j=self.jobs[at].ok_or(Error::Missing)?;
  if j.state!=State::Queued||now>=j.deadline{return Err(Error::Stale);}
  let (ram,cpus)=self.usage();
  if ram.checked_add(j.ram_mb).is_none_or(|r|r>self.ram)||
   cpus.checked_add(j.cpu_slots as u32).is_none_or(|c|c>self.cpus as u32){
   return Err(Error::Full);
  }
  j.state=State::Running;self.jobs[at]=Some(j);Ok(())
 }
 pub fn finish(&mut self,owner:u32,id:u32,success:bool)
  ->Result<(),Error>{
  let at=self.jobs.iter().position(|j|j.is_some_and(|j|j.id==id&&j.owner==owner))
   .ok_or(Error::Missing)?;
  let mut j=self.jobs[at].ok_or(Error::Missing)?;
  if j.state!=State::Running{return Err(Error::Conflict);}
  j.state=if success{State::Done}else{State::Failed};
  self.jobs[at]=Some(j);Ok(())
 }
 pub fn reap(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let at=self.jobs.iter().position(|j|j.is_some_and(|j|j.id==id&&j.owner==owner))
   .ok_or(Error::Missing)?;
  if self.jobs[at].is_some_and(|j|j.state==State::Running){return Err(Error::Conflict);}
  self.jobs[at]=None;Ok(())
 }
 pub fn get(&self,owner:u32,id:u32)->Result<Job,Error>{
  if owner==0{return Err(Error::Denied);}
  self.jobs.iter().flatten().find(|j|j.id==id&&j.owner==owner)
   .copied().ok_or(Error::Missing)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn two_builds_compete_for_ram_and_release_resources(){
  let mut p=Pool::<2>::new(128,2);
  p.submit(1,7,100,2,100).unwrap();
  p.submit(2,7,80,1,100).unwrap();
  p.start(7,1,1).unwrap();assert_eq!(p.usage(),(100,2));
  assert_eq!(p.start(7,2,1),Err(Error::Full));
  p.finish(7,1,true).unwrap();p.start(7,2,1).unwrap();
  assert_eq!(p.usage(),(80,1));
 }
 #[test]fn deadline_and_recycled_task_ids_cannot_preempt_running_build(){
  let mut p=Pool::<1>::new(64,1);p.submit(1,7,20,1,5).unwrap();
  assert_eq!(p.start(7,1,5),Err(Error::Stale));
  assert_eq!(p.submit(1,7,10,1,5),Err(Error::Conflict));
 }
 #[test]fn foreign_owner_cannot_finish_or_reap_job(){
  let mut p=Pool::<1>::new(64,1);p.submit(1,7,20,1,5).unwrap();
  p.start(7,1,1).unwrap();
  assert_eq!(p.finish(8,1,true),Err(Error::Missing));
  assert_eq!(p.reap(7,1),Err(Error::Conflict));
 }
}
