//! R020/06 modular Desktop, Developer, Server and combined service plans.
//! This is policy data; separate installer/boot activation is still required.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Edition{Desktop,Developer,Server,AllInOne}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Service{pub id:u16,pub memory_mb:u16,pub cpu_slots:u8,
 pub required_edition:Edition,pub enabled:bool}
pub struct Profile<const N:usize>{owner:u32,edition:Edition,
 revision:u64,services:[Option<Service>;N],memory_limit:u32,cpu_limit:u16}
impl<const N:usize> Profile<N>{
 pub const fn new(owner:u32,memory_limit:u32,cpu_limit:u16)->Self{
  Self{owner,edition:Edition::Desktop,revision:0,services:[None;N],
   memory_limit,cpu_limit}
 }
 fn check(&self,owner:u32,rev:u64)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if rev!=self.revision{return Err(Error::Stale);}Ok(())
 }
 fn allowed(edition:Edition,required:Edition)->bool{
  edition==Edition::AllInOne||edition==required
 }
 pub fn add(&mut self,owner:u32,rev:u64,id:u16,
  edition:Edition,memory_mb:u16,cpu_slots:u8)->Result<u64,Error>{
  self.check(owner,rev)?;
  if id==0||memory_mb==0||cpu_slots==0||self.services.iter().flatten()
   .any(|s|s.id==id){return Err(Error::Invalid);}
  let pos=self.services.iter().position(Option::is_none).ok_or(Error::Full)?;
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.services[pos]=Some(Service{id,memory_mb,cpu_slots,
   required_edition:edition,enabled:false});self.revision=next;Ok(next)
 }
 pub fn switch(&mut self,owner:u32,rev:u64,edition:Edition)
  ->Result<u64,Error>{
  self.check(owner,rev)?;
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.edition=edition;for slot in &mut self.services{
   if let Some(s)=slot.as_mut(){
    if !Self::allowed(edition,s.required_edition){s.enabled=false;}
   }
  }self.revision=next;Ok(next)
 }
 pub fn enable_batch(&mut self,owner:u32,rev:u64,ids:&[u16])
  ->Result<u64,Error>{
  self.check(owner,rev)?;
  if ids.is_empty()||ids.len()>N{return Err(Error::Invalid);}
  let mut staged=self.services;
  for (i,&id) in ids.iter().enumerate(){
   if id==0||ids[..i].contains(&id){return Err(Error::Invalid);}
   let s=staged.iter_mut().flatten().find(|s|s.id==id)
    .ok_or(Error::Missing)?;
   if !Self::allowed(self.edition,s.required_edition){return Err(Error::Denied);}
   s.enabled=true;
  }
  let mem:u32=staged.iter().flatten().filter(|s|s.enabled)
   .map(|s|s.memory_mb as u32).sum();
  let cpu:u32=staged.iter().flatten().filter(|s|s.enabled)
   .map(|s|s.cpu_slots as u32).sum();
  if mem>self.memory_limit||cpu>self.cpu_limit as u32{return Err(Error::Full);}
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.services=staged;self.revision=next;Ok(next)
 }
 pub fn resource_usage(&self,owner:u32)->Result<(u32,u32),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  Ok((self.services.iter().flatten().filter(|s|s.enabled)
   .map(|s|s.memory_mb as u32).sum(),
   self.services.iter().flatten().filter(|s|s.enabled)
   .map(|s|s.cpu_slots as u32).sum()))
 }
 pub fn edition(&self)->Edition{self.edition}
 pub fn revision(&self)->u64{self.revision}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn developer_profile_enables_only_approved_service_quota(){
  let mut p=Profile::<3>::new(7,128,3);
  p.add(7,0,1,Edition::Developer,64,1).unwrap();
  p.add(7,1,2,Edition::Server,64,1).unwrap();
  p.switch(7,2,Edition::Developer).unwrap();
  assert_eq!(p.enable_batch(7,3,&[1]),Ok(4));
  assert_eq!(p.resource_usage(7),Ok((64,1)));
  assert_eq!(p.enable_batch(7,4,&[2]),Err(Error::Denied));
 }
 #[test]fn combined_edition_refuses_overbudget_atomic_plan(){
  let mut p=Profile::<2>::new(7,100,2);
  p.add(7,0,1,Edition::Desktop,80,1).unwrap();
  p.add(7,1,2,Edition::Server,80,1).unwrap();
  p.switch(7,2,Edition::AllInOne).unwrap();
  assert_eq!(p.enable_batch(7,3,&[1,2]),Err(Error::Full));
  assert_eq!(p.resource_usage(7),Ok((0,0)));
 }
 #[test]fn switching_to_desktop_disables_developer_services(){
  let mut p=Profile::<1>::new(7,100,2);
  p.add(7,0,1,Edition::Developer,20,1).unwrap();
  p.switch(7,1,Edition::Developer).unwrap();p.enable_batch(7,2,&[1]).unwrap();
  p.switch(7,3,Edition::Desktop).unwrap();
  assert_eq!(p.resource_usage(7),Ok((0,0)));
 }
}
