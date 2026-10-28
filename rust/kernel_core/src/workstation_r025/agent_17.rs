//! R025/17: typed bounded debugger watch snapshots, session-authenticated.
//! Values are trusted debugger observations, NOT arbitrary guest memory access.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Type{Boolean,Integer,Utf8,Address}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Watch{pub id:u32,pub ty:Type,pub bytes:[u8;96],
 pub len:usize,pub generation:u64,pub tick:u64}
pub struct Watches<const N:usize>{owner:Identity,values:[Option<Watch>;N],
 last_tick:u64}
impl<const N:usize> Watches<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,values:[None;N],last_tick:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.values.iter().position(|w|w.is_some_and(|v|v.id==id))
 }
 pub fn observe(&mut self,actor:Identity,trusted:bool,id:u32,
  ty:Type,bytes:&[u8],tick:u64)->Result<(),Error>{
  self.auth(actor)?;
  if !trusted||id==0||bytes.is_empty()||bytes.len()>96{
   return Err(Error::Denied);
  }
  if tick<self.last_tick{return Err(Error::Stale);}
  if ty==Type::Boolean&&bytes!=b"true"&&bytes!=b"false"{
   return Err(Error::Invalid);
  }
  if ty==Type::Utf8&&core::str::from_utf8(bytes).is_err(){
   return Err(Error::Invalid);
  }
  let at=self.find(id).or_else(||self.values.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let mut value=Watch{id,ty,bytes:[0;96],len:bytes.len(),
   generation:actor.generation,tick};
  value.bytes[..bytes.len()].copy_from_slice(bytes);
  self.values[at]=Some(value);self.last_tick=tick;Ok(())
 }
 pub fn get(&self,actor:Identity,id:u32,out:&mut[u8])
  ->Result<(Type,usize),Error>{
  self.auth(actor)?;
  let value=self.find(id).and_then(|i|self.values[i]).ok_or(Error::Missing)?;
  if out.len()<value.len{return Err(Error::Full);}
  out[..value.len].copy_from_slice(&value.bytes[..value.len]);
  Ok((value.ty,value.len))
 }
 pub fn remove(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.auth(actor)?;let at=self.find(id).ok_or(Error::Missing)?;
  self.values[at]=None;Ok(())
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  self.auth(actor)?;self.values=[None;N];Ok(())
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.values.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn native_debug_watch_captures_typed_integer_value(){
  let mut w=Watches::<1>::new(id(1));
  w.observe(id(1),true,1,Type::Integer,b"42",1).unwrap();
  let mut out=[0u8;8];
  assert_eq!(w.get(id(1),1,&mut out),Ok((Type::Integer,2)));
  assert_eq!(&out[..2],b"42");
 }
 #[test]fn untrusted_debugger_cannot_inject_fake_variable_value(){
  let mut w=Watches::<1>::new(id(1));
  assert_eq!(w.observe(id(1),false,1,Type::Utf8,b"secret",1),
   Err(Error::Denied));
  assert_eq!(w.count(id(1)),Ok(0));
 }
 #[test]fn recycled_process_identity_cannot_read_previous_debugger_values(){
  let mut w=Watches::<1>::new(id(1));
  w.observe(id(1),true,1,Type::Integer,b"1",1).unwrap();
  assert_eq!(w.get(id(2),1,&mut [0u8;4]),Err(Error::Denied));
 }
}
