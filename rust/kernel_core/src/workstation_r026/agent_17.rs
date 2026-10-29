//! R026/17: trusted debugger variable value changes and snapshots.
//! Compares typed bounded bytes, never reads arbitrary guest memory.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Type{Bool,Int,Text,Pointer}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Value{pub ty:Type,pub data:[u8;96],pub len:usize,pub tick:u64}
pub const EMPTY:Value=Value{ty:Type::Int,data:[0;96],len:0,tick:0};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Change{pub old:Value,pub new:Value,pub changed:bool}
pub struct Watch<const N:usize>{owner:Identity,ids:[Option<u32>;N],
 values:[Value;N],changed:[bool;N]}
impl<const N:usize> Watch<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,ids:[None;N],values:[EMPTY;N],changed:[false;N]}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn index(&self,id:u32)->Option<usize>{
  self.ids.iter().position(|x|*x==Some(id))
 }
 pub fn observe(&mut self,actor:Identity,trusted:bool,id:u32,
  ty:Type,data:&[u8],tick:u64)->Result<bool,Error>{
  self.auth(actor)?;
  if !trusted||id==0||data.is_empty()||data.len()>96{return Err(Error::Denied);}
  if ty==Type::Text&&core::str::from_utf8(data).is_err(){return Err(Error::Invalid);}
  let at=self.index(id).or_else(||self.ids.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  if self.ids[at].is_some()&&tick<self.values[at].tick{return Err(Error::Stale);}
  let mut next=Value{ty,data:[0;96],len:data.len(),tick};
  next.data[..data.len()].copy_from_slice(data);
  let changed=self.ids[at].is_some()&&
   (self.values[at].ty!=ty||
    self.values[at].data[..self.values[at].len]!=next.data[..next.len]);
  self.ids[at]=Some(id);self.values[at]=next;self.changed[at]=changed;
  Ok(changed)
 }
 pub fn get(&self,actor:Identity,id:u32)->Result<(Value,bool),Error>{
  self.auth(actor)?;
  let at=self.index(id).ok_or(Error::Missing)?;
  Ok((self.values[at],self.changed[at]))
 }
 pub fn compare(&self,actor:Identity,id:u32,
  previous:Value)->Result<Change,Error>{
  let (new,_)=self.get(actor,id)?;
  Ok(Change{old:previous,new,changed:previous.ty!=new.ty||
    previous.data[..previous.len]!=new.data[..new.len]})
 }
 pub fn remove(&mut self,actor:Identity,id:u32)->Result<(),Error>{
  self.auth(actor)?;let at=self.index(id).ok_or(Error::Missing)?;
  self.ids[at]=None;self.values[at]=EMPTY;self.changed[at]=false;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn debugger_detects_changed_live_variable_after_trusted_step(){
  let mut w=Watch::<1>::new(id(1));
  assert_eq!(w.observe(id(1),true,1,Type::Int,b"1",1),Ok(false));
  let old=w.get(id(1),1).unwrap().0;
  assert_eq!(w.observe(id(1),true,1,Type::Int,b"2",2),Ok(true));
  assert!(w.compare(id(1),1,old).unwrap().changed);
 }
 #[test]fn same_value_update_does_not_generate_false_changed_state(){
  let mut w=Watch::<1>::new(id(1));
  w.observe(id(1),true,1,Type::Text,b"same",1).unwrap();
  assert_eq!(w.observe(id(1),true,1,Type::Text,b"same",2),Ok(false));
 }
 #[test]fn foreign_user_and_untrusted_debugger_cannot_replace_value(){
  let mut w=Watch::<1>::new(id(1));
  assert_eq!(w.observe(id(2),true,1,Type::Int,b"1",1),Err(Error::Denied));
  assert_eq!(w.observe(id(1),false,1,Type::Int,b"1",1),Err(Error::Denied));
 }
}
