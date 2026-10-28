//! R021/18 signed toolchain catalog pin/upgrade/rollback state.
//! Signer verification must be performed by trusted caller; no package downloader.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Channel{Stable,Beta,Experimental}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Toolchain{pub id:u32,pub version:u64,pub abi:u16,
 pub channel:Channel,pub digest:[u8;32],pub verified:bool}
pub struct Catalog<const N:usize>{installed:[Option<Toolchain>;N],
 pins:[Option<Toolchain>;N],owner:u32}
impl<const N:usize> Catalog<N>{
 pub const fn new(owner:u32)->Self{Self{installed:[None;N],pins:[None;N],owner}}
 fn auth(&self,owner:u32)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn add_verified(&mut self,supervisor:bool,item:Toolchain)
  ->Result<(),Error>{
  if !supervisor||!item.verified||item.id==0||item.version==0||
   item.abi==0||item.digest==[0;32]{return Err(Error::Denied);}
  if self.installed.iter().flatten().any(|p|p.id==item.id&&p.version==item.version){
   return Err(Error::Conflict);
  }
  let at=self.installed.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.installed[at]=Some(item);Ok(())
 }
 pub fn pin(&mut self,owner:u32,id:u32,version:u64,
  allowed:Channel)->Result<Toolchain,Error>{
  self.auth(owner)?;
  let item=self.installed.iter().flatten().find(|p|p.id==id&&p.version==version)
   .copied().ok_or(Error::Missing)?;
  if item.channel!=allowed{return Err(Error::Denied);}
  let at=self.pins.iter().position(|p|p.is_some_and(|p|p.id==id))
   .or_else(||self.pins.iter().position(Option::is_none)).ok_or(Error::Full)?;
  self.pins[at]=Some(item);Ok(item)
 }
 pub fn resolve(&self,owner:u32,id:u32,min_abi:u16)
  ->Result<Toolchain,Error>{
  self.auth(owner)?;
  let item=self.pins.iter().flatten().find(|p|p.id==id)
   .copied().ok_or(Error::Missing)?;
  if item.abi<min_abi||!item.verified{return Err(Error::Denied);}
  Ok(item)
 }
 pub fn unpin(&mut self,owner:u32,id:u32)->Result<(),Error>{
  self.auth(owner)?;
  let at=self.pins.iter().position(|p|p.is_some_and(|p|p.id==id))
   .ok_or(Error::Missing)?;
  self.pins[at]=None;Ok(())
 }
 pub fn pinned_count(&self,owner:u32)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.pins.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn tc(v:u64,c:Channel)->Toolchain{
  Toolchain{id:1,version:v,abi:1,channel:c,digest:[v as u8;32],verified:true}
 }
 #[test]fn selects_verified_stable_toolchain_version_and_resolves_abi(){
  let mut c=Catalog::<2>::new(7);
  c.add_verified(true,tc(17,Channel::Stable)).unwrap();
  c.add_verified(true,tc(21,Channel::Beta)).unwrap();
  c.pin(7,1,17,Channel::Stable).unwrap();
  assert_eq!(c.resolve(7,1,1).unwrap().version,17);
 }
 #[test]fn beta_toolchain_requires_explicit_channel_permission(){
  let mut c=Catalog::<1>::new(7);
  c.add_verified(true,tc(21,Channel::Beta)).unwrap();
  assert_eq!(c.pin(7,1,21,Channel::Stable),Err(Error::Denied));
  assert_eq!(c.pin(7,1,21,Channel::Beta).unwrap().version,21);
 }
 #[test]fn unverified_artifact_and_foreign_user_cannot_pin_toolchain(){
  let mut c=Catalog::<1>::new(7);
  assert_eq!(c.add_verified(false,tc(17,Channel::Stable)),Err(Error::Denied));
  c.add_verified(true,tc(17,Channel::Stable)).unwrap();
  assert_eq!(c.pin(8,1,17,Channel::Stable),Err(Error::Denied));
 }
}
