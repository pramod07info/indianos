//! R026/10: secure generation-isolated guest terminal environment and expansion.
//! Stores session-local variables; no host environment is accessed.
use super::Error;
use crate::batch5_session::Identity;
pub const KEY:usize=32;pub const VALUE:usize=96;
#[derive(Clone,Copy)]
pub struct Var{pub key:[u8;KEY],pub klen:usize,
 pub value:[u8;VALUE],pub vlen:usize}
pub struct Env<const N:usize>{owner:Identity,vars:[Option<Var>;N]}
impl<const N:usize> Env<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,vars:[None;N]}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,key:&[u8])->Option<usize>{
  self.vars.iter().position(|v|v.is_some_and(|x|&x.key[..x.klen]==key))
 }
 pub fn set(&mut self,actor:Identity,key:&[u8],value:&[u8])->Result<(),Error>{
  self.auth(actor)?;
  if key.is_empty()||key.len()>KEY||value.len()>VALUE||
   !key[0].is_ascii_alphabetic()||
   !key.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_')||
   value.iter().any(|b|*b==0||*b==b'\n'||*b==0x1b)||
   key.windows(6).any(|w|w==b"SECRET"){return Err(Error::Invalid);}
  let at=self.find(key).or_else(||self.vars.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let mut var=Var{key:[0;KEY],klen:key.len(),
   value:[0;VALUE],vlen:value.len()};
  var.key[..key.len()].copy_from_slice(key);
  var.value[..value.len()].copy_from_slice(value);self.vars[at]=Some(var);Ok(())
 }
 pub fn get(&self,actor:Identity,key:&[u8],
  out:&mut[u8])->Result<usize,Error>{
  self.auth(actor)?;
  let var=self.find(key).and_then(|i|self.vars[i]).ok_or(Error::Missing)?;
  if var.vlen>out.len(){return Err(Error::Full);}
  out[..var.vlen].copy_from_slice(&var.value[..var.vlen]);Ok(var.vlen)
 }
 pub fn unset(&mut self,actor:Identity,key:&[u8])->Result<(),Error>{
  self.auth(actor)?;
  let at=self.find(key).ok_or(Error::Missing)?;
  self.vars[at]=None;Ok(())
 }
 pub fn expand(&self,actor:Identity,template:&[u8],
  out:&mut[u8])->Result<usize,Error>{
  self.auth(actor)?;
  let mut stage=[0u8;512];let mut n=0;let mut i=0;
  while i<template.len(){
   if template[i]==b'$'&&template.get(i+1)==Some(&b'{'){
    let end=template[i+2..].iter().position(|&x|x==b'}')
     .map(|x|x+i+2).ok_or(Error::Invalid)?;
    let key=&template[i+2..end];
    let mut val=[0u8;VALUE];let len=self.get(actor,key,&mut val)?;
    if n+len>stage.len(){return Err(Error::Full);}
    stage[n..n+len].copy_from_slice(&val[..len]);n+=len;i=end+1;
   }else{
    if n==stage.len(){return Err(Error::Full);}
    stage[n]=template[i];n+=1;i+=1;
   }
  }
  if n>out.len(){return Err(Error::Full);}
  out[..n].copy_from_slice(&stage[..n]);Ok(n)
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.vars.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn resolves_guest_terminal_project_env_and_expands_literal_path(){
  let mut env=Env::<2>::new(id(1));env.set(id(1),b"PROJECT",b"alpha").unwrap();
  let mut out=[0;32];
  let n=env.expand(id(1),b"/\${PROJECT}/src",&mut out).unwrap();
  assert_eq!(&out[..n],b"/alpha/src");
  assert_eq!(env.count(id(1)),Ok(1));
 }
 #[test]fn recycled_user_session_is_denied_access_to_environment(){
  let mut env=Env::<1>::new(id(1));env.set(id(1),b"FOO",b"bar").unwrap();
  assert_eq!(env.expand(id(2),b"\${FOO}",&mut [0;8]),Err(Error::Denied));
 }
 #[test]fn command_control_bytes_and_sensitive_keys_fail_validation(){
  let mut env=Env::<1>::new(id(1));
  assert_eq!(env.set(id(1),b"SECRET_TOKEN",b"x"),Err(Error::Invalid));
  assert_eq!(env.set(id(1),b"FOO",b"a\nb"),Err(Error::Invalid));
 }
}
