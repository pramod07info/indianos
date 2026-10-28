//! R019/15 signed extension capabilities and explicit developer consent.
//! This enforces registration policy; no dynamic plugin ABI is active.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Extension{pub id:u32,pub owner:u32,pub version:u64,
 pub requested:u32,pub approved:u32,pub signed:bool,pub enabled:bool}
pub struct Registry<const N:usize>{items:[Option<Extension>;N]}
impl<const N:usize> Registry<N>{
 pub const fn new()->Self{Self{items:[None;N]}}
 fn index(&self,owner:u32,id:u32)->Option<usize>{
  self.items.iter().position(|x|x.is_some_and(|e|e.owner==owner&&e.id==id))
 }
 pub fn install(&mut self,owner:u32,id:u32,version:u64,
  rights:u32,verified_signature:bool)->Result<(),Error>{
  if owner==0||id==0||version==0||rights==0||
   rights&!crate::batch6_security::ALL!=0{return Err(Error::Invalid);}
  if !verified_signature{return Err(Error::Denied);}
  if self.index(owner,id).is_some(){return Err(Error::Conflict);}
  let slot=self.items.iter_mut().find(|x|x.is_none()).ok_or(Error::Full)?;
  *slot=Some(Extension{id,owner,version,requested:rights,approved:0,
    signed:true,enabled:false});Ok(())
 }
 pub fn approve(&mut self,owner:u32,id:u32,mask:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut e=self.items[i].ok_or(Error::Missing)?;
  if mask&e.requested!=mask{return Err(Error::Denied);}
  e.approved=mask;e.enabled=false;self.items[i]=Some(e);Ok(())
 }
 pub fn enable(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut e=self.items[i].ok_or(Error::Missing)?;
  if !e.signed||e.approved!=e.requested{return Err(Error::Denied);}
  e.enabled=true;self.items[i]=Some(e);Ok(())
 }
 pub fn disable(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut e=self.items[i].ok_or(Error::Missing)?;
  e.enabled=false;self.items[i]=Some(e);Ok(())
 }
 pub fn revoke(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;self.items[i]=None;Ok(())
 }
 pub fn can_call(&self,owner:u32,id:u32,required:u32)->bool{
  self.index(owner,id).and_then(|i|self.items[i]).is_some_and(
   |e|e.enabled&&required!=0&&e.approved&required==required)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch6_security::{VIEW,FILE_READ};
 #[test]fn signed_extension_still_requires_full_explicit_permissions(){
  let mut r=Registry::<1>::new();
  r.install(10,1,1,VIEW|FILE_READ,true).unwrap();
  assert_eq!(r.enable(10,1),Err(Error::Denied));
  r.approve(10,1,VIEW).unwrap();assert_eq!(r.enable(10,1),Err(Error::Denied));
  r.approve(10,1,VIEW|FILE_READ).unwrap();r.enable(10,1).unwrap();
  assert!(r.can_call(10,1,VIEW));r.disable(10,1).unwrap();assert!(!r.can_call(10,1,VIEW));
 }
 #[test]fn invalid_signature_fails_closed_and_does_not_consume_slot(){
  let mut r=Registry::<1>::new();
  assert_eq!(r.install(10,1,1,VIEW,false),Err(Error::Denied));
  assert_eq!(r.install(10,1,1,VIEW,true),Ok(()));
 }
 #[test]fn foreign_owner_cannot_grant_or_execute_plugin(){
  let mut r=Registry::<1>::new();r.install(10,1,1,VIEW,true).unwrap();
  assert_eq!(r.approve(11,1,VIEW),Err(Error::Missing));
  assert!(!r.can_call(11,1,VIEW));assert_eq!(r.revoke(11,1),Err(Error::Missing));
 }
}
