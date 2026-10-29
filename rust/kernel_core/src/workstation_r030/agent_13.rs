//! R030/13: bounded per-session terminal environment (never shell eval).
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Var {pub key:[u8;16],pub k:usize,pub value:[u8;64],pub n:usize}
pub const EMPTY:Var=Var{key:[0;16],k:0,value:[0;64],n:0};
pub struct Env<const N:usize>{owner:u64,vars:[Var;N]}
impl<const N:usize> Env<N>{
 pub fn new(owner:u64)->Self{Self{owner,vars:[EMPTY;N]}}
 pub fn set(&mut self,owner:u64,key:&[u8],value:&[u8])->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if key.is_empty()||key.len()>16||value.len()>64||
   !key[0].is_ascii_alphabetic()||
   !key.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_')||
   value.contains(&0){return Err(Error::Invalid);}
  let i=self.vars.iter().position(|v|v.k==key.len()&&&v.key[..v.k]==key)
   .or_else(||self.vars.iter().position(|v|v.k==0)).ok_or(Error::Full)?;
  let mut entry=EMPTY;entry.key[..key.len()].copy_from_slice(key);
  entry.value[..value.len()].copy_from_slice(value);
  entry.k=key.len();entry.n=value.len();self.vars[i]=entry;Ok(())
 }
 pub fn read(&self,owner:u64,key:&[u8],out:&mut[u8])->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let item=self.vars.iter().find(|v|v.k==key.len()&&&v.key[..v.k]==key)
   .ok_or(Error::Missing)?;
  if item.n>out.len(){return Err(Error::Full);}
  out[..item.n].copy_from_slice(&item.value[..item.n]);Ok(item.n)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn terminal_environment_updates_selected_key_without_leaking(){
  let mut env=Env::<1>::new(7);env.set(7,b"HOME",b"/p").unwrap();
  env.set(7,b"HOME",b"/p/src").unwrap();
  let mut out=[0;8];let n=env.read(7,b"HOME",&mut out).unwrap();
  assert_eq!(&out[..n],b"/p/src");
 }
 #[test]fn foreign_session_and_injection_like_keys_are_rejected(){
  let mut env=Env::<1>::new(7);
  assert_eq!(env.set(8,b"HOME",b"/p"),Err(Error::Denied));
  assert_eq!(env.set(7,b"HOME;rm",b"/p"),Err(Error::Invalid));
 }
}