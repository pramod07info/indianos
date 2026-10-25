//! R038/32: explicit consent bound to app, owner epoch and package digest.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Decision{Pending,Granted,Denied}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Error{Invalid,Stale,WrongState,Denied}
pub struct Consent{owner:u32,epoch:u64,digest:[u8;32],permissions:u32,decision:Decision}
impl Consent{
 pub fn new(owner:u32,epoch:u64,digest:[u8;32],permissions:u32)->Result<Self,Error>{
  if owner==0||epoch==0||digest==[0;32]{return Err(Error::Invalid);}
  Ok(Self{owner,epoch,digest,permissions,decision:Decision::Pending})
 }
 pub fn respond(&mut self,owner:u32,epoch:u64,digest:[u8;32],approved:bool,granted:u32)->Result<(),Error>{
  if self.owner!=owner||self.epoch!=epoch||self.digest!=digest{return Err(Error::Stale);}
  if self.decision!=Decision::Pending{return Err(Error::WrongState);}
  if !approved||self.permissions&!granted!=0{
   self.decision=Decision::Denied;return Err(Error::Denied);
  }
  self.decision=Decision::Granted;Ok(())
 }
 pub fn authorized(&self,owner:u32,epoch:u64,digest:[u8;32])->bool{
  self.decision==Decision::Granted&&self.owner==owner&&self.epoch==epoch&&self.digest==digest
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn epoch_and_digest_binding(){
  let mut c=Consent::new(1,2,[4;32],8).unwrap();
  assert_eq!(c.respond(1,3,[4;32],true,8),Err(Error::Stale));
  c.respond(1,2,[4;32],true,8).unwrap();
  assert!(!c.authorized(1,3,[4;32]));assert!(c.authorized(1,2,[4;32]));
 }
}