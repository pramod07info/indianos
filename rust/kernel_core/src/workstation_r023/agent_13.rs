//! R023/13: process/session environment variables and scoped working paths.
//! No dynamic allocation or host environment access; generation-safe ownership.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub key:[u8;32],pub key_len:usize,
 pub value:[u8;128],pub value_len:usize}
pub const EMPTY:Entry=Entry{key:[0;32],key_len:0,value:[0;128],value_len:0};
pub struct Environment<const N:usize>{
 owner:Identity,entries:[Option<Entry>;N],revision:u64
}
impl<const N:usize> Environment<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,entries:[None;N],revision:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,key:&[u8])->Option<usize>{
  self.entries.iter().position(|v|v.is_some_and(|x|
   &x.key[..x.key_len]==key))
 }
 pub fn set(&mut self,actor:Identity,key:&[u8],value:&[u8],
  revision:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if revision!=self.revision{return Err(Error::Stale);}
  if key.is_empty()||key.len()>32||value.len()>128||
   !key.iter().enumerate().all(|(i,b)|
    b.is_ascii_uppercase()||*b==b'_'||(i>0&&b.is_ascii_digit()))||
   value.iter().any(|b|*b==0){return Err(Error::Invalid);}
  let at=self.find(key).or_else(||self.entries.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let next=revision.checked_add(1).ok_or(Error::Overflow)?;
  let mut item=EMPTY;
  item.key[..key.len()].copy_from_slice(key);item.key_len=key.len();
  item.value[..value.len()].copy_from_slice(value);item.value_len=value.len();
  self.entries[at]=Some(item);self.revision=next;Ok(next)
 }
 pub fn get(&self,actor:Identity,key:&[u8],out:&mut[u8])
  ->Result<usize,Error>{
  self.auth(actor)?;
  let item=self.find(key).and_then(|i|self.entries[i]).ok_or(Error::Missing)?;
  if out.len()<item.value_len{return Err(Error::Full);}
  out[..item.value_len].copy_from_slice(&item.value[..item.value_len]);
  Ok(item.value_len)
 }
 pub fn unset(&mut self,actor:Identity,key:&[u8],revision:u64)
  ->Result<u64,Error>{
  self.auth(actor)?;
  if revision!=self.revision{return Err(Error::Stale);}
  let i=self.find(key).ok_or(Error::Missing)?;
  let next=revision.checked_add(1).ok_or(Error::Overflow)?;
  self.entries[i]=None;self.revision=next;Ok(next)
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.entries.iter().flatten().count())
 }
 pub fn revision(&self)->u64{self.revision}
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  self.auth(actor)?;self.entries=[None;N];self.revision+=1;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn process_environment_carries_build_toolchain_and_editor_values(){
  let mut e=Environment::<3>::new(id(1));
  e.set(id(1),b"RUST_TARGET",b"x86_64",0).unwrap();
  e.set(id(1),b"EDITOR",b"indianos",1).unwrap();
  let mut out=[0u8;32];let n=e.get(id(1),b"EDITOR",&mut out).unwrap();
  assert_eq!(&out[..n],b"indianos");assert_eq!(e.count(id(1)),Ok(2));
 }
 #[test]fn reused_pid_generation_cannot_read_or_rewrite_user_secrets(){
  let mut e=Environment::<1>::new(id(1));
  e.set(id(1),b"TOKEN",b"private",0).unwrap();
  assert_eq!(e.get(id(2),b"TOKEN",&mut [0u8;32]),Err(Error::Denied));
  assert_eq!(e.unset(id(2),b"TOKEN",1),Err(Error::Denied));
 }
 #[test]fn insufficient_output_and_stale_set_preserve_old_value(){
  let mut e=Environment::<1>::new(id(1));e.set(id(1),b"A",b"secret",0).unwrap();
  let mut out=[55u8;3];
  assert_eq!(e.get(id(1),b"A",&mut out),Err(Error::Full));
  assert_eq!(out,[55;3]);
  assert_eq!(e.set(id(1),b"A",b"new",0),Err(Error::Stale));
 }
}
