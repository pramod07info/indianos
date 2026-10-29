//! R028/16: safe source breakpoint condition evaluation on trusted integer values.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Comparator{Eq,Ne,Lt,Le,Gt,Ge}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Condition{pub id:u32,pub op:Comparator,pub value:i64,pub enabled:bool,
 pub hits:u32,pub hit_limit:u32}
pub struct Conditions<const N:usize>{owner:Identity,items:[Option<Condition>;N]}
impl<const N:usize> Conditions<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.items.iter().position(|x|x.is_some_and(|c|c.id==id))
 }
 pub fn add(&mut self,who:Identity,id:u32,op:Comparator,value:i64,
 hit_limit:u32)->Result<(),Error>{
  self.auth(who)?;
  if id==0||hit_limit==0||self.slot(id).is_some(){return Err(Error::Invalid);}
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[i]=Some(Condition{id,op,value,enabled:true,hits:0,hit_limit});Ok(())
 }
 pub fn check(&mut self,who:Identity,trusted_debugger:bool,id:u32,
 actual:i64)->Result<bool,Error>{
  self.auth(who)?;if !trusted_debugger{return Err(Error::Denied);}
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut c=self.items[i].ok_or(Error::Missing)?;
  if !c.enabled{return Ok(false);}
  let matching=match c.op{
   Comparator::Eq=>actual==c.value,Comparator::Ne=>actual!=c.value,
   Comparator::Lt=>actual<c.value,Comparator::Le=>actual<=c.value,
   Comparator::Gt=>actual>c.value,Comparator::Ge=>actual>=c.value
  };
  if matching{c.hits=c.hits.checked_add(1).ok_or(Error::Overflow)?;}
  self.items[i]=Some(c);Ok(matching&&c.hits>=c.hit_limit)
 }
 pub fn remove(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let i=self.slot(id).ok_or(Error::Missing)?;self.items[i]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn conditional_breakpoint_triggers_only_after_second_matching_hit(){
  let mut s=Conditions::<1>::new(id(1));s.add(id(1),1,Comparator::Gt,10,2).unwrap();
  assert_eq!(s.check(id(1),true,1,5),Ok(false));
  assert_eq!(s.check(id(1),true,1,20),Ok(false));
  assert_eq!(s.check(id(1),true,1,20),Ok(true));
 }
 #[test]fn untrusted_debugger_and_recycled_pid_cannot_inject_hits(){
  let mut s=Conditions::<1>::new(id(1));s.add(id(1),1,Comparator::Eq,2,1).unwrap();
  assert_eq!(s.check(id(1),false,1,2),Err(Error::Denied));
  assert_eq!(s.check(id(2),true,1,2),Err(Error::Denied));
 }
}