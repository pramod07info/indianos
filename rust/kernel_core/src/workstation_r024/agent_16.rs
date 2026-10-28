//! R024/16: owner-and-PID-generation file write leases for developer documents.
//! Kernel ingress must call acquire/check before every privileged file write.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Lock{pub owner:Identity,pub path:[u8;PATH_MAX],
 pub len:usize,pub file_rev:u64,pub expires:u64,pub token:u64}
pub struct Locks<const N:usize>{entries:[Option<Lock>;N],sequence:u64}
impl<const N:usize> Locks<N>{
 pub const fn new()->Self{Self{entries:[None;N],sequence:0}}
 fn index(&self,path:&[u8])->Option<usize>{
  self.entries.iter().position(|x|x.is_some_and(|l|&l.path[..l.len]==path))
 }
 pub fn acquire<const F:usize>(&mut self,owner:Identity,fs:&RamFs<F>,
 path:&[u8],now:u64,ttl:u64)->Result<u64,Error>{
  if owner.pid==0||owner.generation==0||path.len()<2||
   path.len()>PATH_MAX||ttl==0{return Err(Error::Invalid);}
  let (revision,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if let Some(i)=self.index(path){
   if self.entries[i].is_some_and(|l|l.expires>now){
    return Err(Error::Conflict);
   }
  }
  let slot=self.index(path).or_else(||self.entries.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let token=self.sequence.checked_add(1).ok_or(Error::Overflow)?;
  let expires=now.checked_add(ttl).ok_or(Error::Overflow)?;
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  self.entries[slot]=Some(Lock{owner,path:p,len:path.len(),
   file_rev:revision,expires,token});
  self.sequence=token;Ok(token)
 }
 pub fn check<const F:usize>(&self,owner:Identity,fs:&RamFs<F>,
  path:&[u8],token:u64,now:u64)->Result<(),Error>{
  if owner.pid==0{return Err(Error::Denied);}
  let lock=self.index(path).and_then(|i|self.entries[i]).ok_or(Error::Missing)?;
  if lock.owner!=owner||token!=lock.token||now>=lock.expires{
   return Err(Error::Denied);
  }
  let (version,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if version!=lock.file_rev{return Err(Error::Stale);}
  Ok(())
 }
 pub fn release(&mut self,owner:Identity,path:&[u8],
  token:u64)->Result<(),Error>{
  let index=self.index(path).ok_or(Error::Missing)?;
  let l=self.entries[index].ok_or(Error::Missing)?;
  if owner.pid==0||l.owner!=owner||l.token!=token{
   return Err(Error::Denied);
  }
  self.entries[index]=None;Ok(())
 }
 pub fn revoke_session(&mut self,supervisor:bool,owner:Identity)
  ->Result<usize,Error>{
  if !supervisor{return Err(Error::Denied);}
  let mut count=0;
  for l in &mut self.entries{
   if l.is_some_and(|v|v.owner==owner){*l=None;count+=1;}
  }Ok(count)
 }
 pub fn count(&self)->usize{self.entries.iter().flatten().count()}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn real_source_write_lease_has_owner_revision_and_expiration(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"v1").unwrap();
  let mut locks=Locks::<1>::new();
  let t=locks.acquire(id(1),&fs,b"/a",1,10).unwrap();
  assert_eq!(locks.check(id(1),&fs,b"/a",t,2),Ok(()));
  assert_eq!(locks.check(id(1),&fs,b"/a",t,11),Err(Error::Denied));
 }
 #[test]fn recycled_user_identity_cannot_hijack_file_lock(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let mut l=Locks::<1>::new();let token=l.acquire(id(1),&fs,b"/a",1,10).unwrap();
  assert_eq!(l.check(id(2),&fs,b"/a",token,2),Err(Error::Denied));
 }
 #[test]fn external_edit_invalidates_write_lease_until_reacquired(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let mut l=Locks::<1>::new();let token=l.acquire(id(1),&fs,b"/a",1,10).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(l.check(id(1),&fs,b"/a",token,2),Err(Error::Stale));
 }
}
