//! R022/18: bounded workstation memory/CPU usage governor with priorities.
//! Trusted resource accounting is external; this plans safe admissions and caps.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Priority{Background,Normal,Interactive}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tenant{pub owner:u32,pub app:u32,pub memory:u32,pub cpu:u16,
 pub priority:Priority,pub active:bool}
pub struct Governor<const N:usize>{items:[Option<Tenant>;N],
 max_memory:u32,max_cpu:u16,interactive_reserve:u32}
impl<const N:usize> Governor<N>{
 pub const fn new(mem:u32,cpu:u16,interactive_reserve:u32)->Self{
  Self{items:[None;N],max_memory:mem,max_cpu:cpu,interactive_reserve}
 }
 pub fn usage(&self)->(u32,u32){
  self.items.iter().flatten().filter(|r|r.active)
   .fold((0,0),|(m,c),r|(m+r.memory,c+r.cpu as u32))
 }
 pub fn admit(&mut self,tenant:Tenant)->Result<(),Error>{
  if tenant.owner==0||tenant.app==0||tenant.memory==0||tenant.cpu==0{
   return Err(Error::Invalid);
  }
  if self.items.iter().flatten().any(|r|r.owner==tenant.owner&&r.app==tenant.app){
   return Err(Error::Conflict);
  }
  let (mem,cpu)=self.usage();
  let max_mem=if tenant.priority==Priority::Background{
   self.max_memory.saturating_sub(self.interactive_reserve)
  }else{self.max_memory};
  if mem.checked_add(tenant.memory).is_none_or(|n|n>max_mem)||
   cpu.checked_add(tenant.cpu as u32).is_none_or(|n|n>self.max_cpu as u32){
   return Err(Error::Full);
  }
  let at=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut allowed=tenant;allowed.active=true;
  self.items[at]=Some(allowed);Ok(())
 }
 pub fn release(&mut self,owner:u32,app:u32)->Result<(),Error>{
  let at=self.items.iter().position(|r|r.is_some_and(|t|
   t.owner==owner&&t.app==app)).ok_or(Error::Missing)?;
  self.items[at]=None;Ok(())
 }
 pub fn shrink(&mut self,owner:u32,app:u32,
  memory:u32,cpu:u16)->Result<(),Error>{
  if memory==0||cpu==0{return Err(Error::Invalid);}
  let at=self.items.iter().position(|r|r.is_some_and(|t|
   t.owner==owner&&t.app==app)).ok_or(Error::Missing)?;
  let mut t=self.items[at].ok_or(Error::Missing)?;
  if memory>t.memory||cpu>t.cpu{return Err(Error::Conflict);}
  t.memory=memory;t.cpu=cpu;self.items[at]=Some(t);Ok(())
 }
 pub fn remaining(&self)->(u32,u16){
  let (mem,cpu)=self.usage();
  (self.max_memory.saturating_sub(mem),
   self.max_cpu.saturating_sub(cpu as u16))
 }
 pub fn count(&self)->usize{self.items.iter().flatten().count()}
}
#[cfg(test)]mod tests{
 use super::*;
 fn tenant(app:u32,mem:u32,p:Priority)->Tenant{
  Tenant{owner:7,app,memory:mem,cpu:1,priority:p,active:false}
 }
 #[test]fn developer_compiler_cannot_exhaust_interactive_desktop_reserve(){
  let mut g=Governor::<2>::new(128,2,32);
  g.admit(tenant(1,80,Priority::Background)).unwrap();
  assert_eq!(g.admit(tenant(2,32,Priority::Background)),Err(Error::Full));
  g.admit(tenant(2,32,Priority::Interactive)).unwrap();
  assert_eq!(g.usage(),(112,2));
 }
 #[test]fn release_frees_all_task_memory_for_new_workloads(){
  let mut g=Governor::<1>::new(64,1,16);
  g.admit(tenant(1,40,Priority::Normal)).unwrap();
  g.release(7,1).unwrap();
  assert_eq!(g.remaining(),(64,1));
 }
 #[test]fn invalid_shrink_cannot_increase_memory_allocation(){
  let mut g=Governor::<1>::new(64,1,8);
  g.admit(tenant(1,32,Priority::Normal)).unwrap();
  assert_eq!(g.shrink(7,1,48,1),Err(Error::Conflict));
  assert_eq!(g.usage(),(32,1));
 }
}
