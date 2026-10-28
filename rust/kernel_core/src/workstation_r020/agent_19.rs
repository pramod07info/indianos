//! R020/19 trusted workstation sessions and generation-safe lifecycle.
//! A supervisor must invoke teardown hooks in every real subsystem.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Session{pub owner:Identity,pub expires:u64,pub projects:u16,
 pub windows:u16,pub queued_jobs:u16,pub active:bool}
pub struct Registry<const N:usize>{sessions:[Option<Session>;N]}
impl<const N:usize> Registry<N>{
 pub const fn new()->Self{Self{sessions:[None;N]}}
 fn locate(&self,owner:Identity)->Option<usize>{
  self.sessions.iter().position(|x|x.is_some_and(|e|e.owner==owner))
 }
 pub fn login(&mut self,owner:Identity,now:u64,ttl:u64)
  ->Result<(),Error>{
  if owner.pid==0||owner.generation==0||ttl==0{return Err(Error::Invalid);}
  let expires=now.checked_add(ttl).ok_or(Error::Overflow)?;
  if self.locate(owner).is_some(){return Err(Error::Conflict);}
  if self.sessions.iter().flatten().any(|s|s.owner.pid==owner.pid&&s.active){
   return Err(Error::Denied);
  }
  let slot=self.sessions.iter_mut().find(|x|x.is_none()).ok_or(Error::Full)?;
  *slot=Some(Session{owner,expires,projects:0,windows:0,
   queued_jobs:0,active:true});Ok(())
 }
 pub fn resource_claim(&mut self,owner:Identity,now:u64,
   projects:u16,windows:u16,jobs:u16)->Result<(),Error>{
  let i=self.locate(owner).ok_or(Error::Denied)?;
  let mut s=self.sessions[i].ok_or(Error::Denied)?;
  if !s.active||now>=s.expires{return Err(Error::Denied);}
  s.projects=s.projects.checked_add(projects).ok_or(Error::Overflow)?;
  s.windows=s.windows.checked_add(windows).ok_or(Error::Overflow)?;
  s.queued_jobs=s.queued_jobs.checked_add(jobs).ok_or(Error::Overflow)?;
  self.sessions[i]=Some(s);Ok(())
 }
 pub fn active(&self,owner:Identity,now:u64)->bool{
  self.locate(owner).and_then(|i|self.sessions[i])
   .is_some_and(|s|s.active&&now<s.expires)
 }
 pub fn retire(&mut self,owner:Identity)->Result<Session,Error>{
  let i=self.locate(owner).ok_or(Error::Denied)?;
  let s=self.sessions[i].ok_or(Error::Denied)?;
  self.sessions[i]=None;Ok(s)
 }
 pub fn expire(&mut self,now:u64,out:&mut[Identity])
  ->Result<usize,Error>{
  let n=self.sessions.iter().flatten().filter(|s|now>=s.expires).count();
  if n>out.len(){return Err(Error::Full);}
  let mut i=0;for s in &mut self.sessions{
   if s.is_some_and(|session|now>=session.expires){
    out[i]=s.take().ok_or(Error::Missing)?.owner;i+=1;
   }
  }Ok(i)
 }
 pub fn count(&self)->usize{self.sessions.iter().flatten().count()}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn creates_project_editor_and_task_session_then_retires(){
  let mut s=Registry::<1>::new();s.login(id(1),1,10).unwrap();
  s.resource_claim(id(1),2,1,2,3).unwrap();
  assert_eq!(s.retire(id(1)).unwrap().queued_jobs,3);
  assert!(!s.active(id(1),2));assert_eq!(s.count(),0);
 }
 #[test]fn reused_pid_is_denied until_previous_session_is_reaped(){
  let mut s=Registry::<1>::new();s.login(id(1),1,10).unwrap();
  assert_eq!(s.login(id(2),2,10),Err(Error::Denied));
  s.retire(id(1)).unwrap();assert_eq!(s.login(id(2),2,10),Ok(()));
 }
 #[test]fn short_expiration_list_does_not_drop_session(){
  let mut s=Registry::<2>::new();
  s.login(id(1),1,2).unwrap();
  let peer=Identity{pid:8,generation:1};s.login(peer,1,2).unwrap();
  let mut out=[id(9);1];
  assert_eq!(s.expire(3,&mut out),Err(Error::Full));
  assert_eq!(s.count(),2);assert_eq!(out,[id(9)]);
 }
}
