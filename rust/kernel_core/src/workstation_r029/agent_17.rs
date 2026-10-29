//! R029/17: safe arithmetic watch expressions from trusted debugger observations.
//! No arbitrary evaluation, memory reads or scripts; typed int64 only.
use super::Error;
use crate::batch5_session::Identity;
use crate::workstation_r027::agent_17 as arithmetic;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Watch{pub id:u32,pub previous:i64,pub current:i64,
 pub tick:u64,pub has_previous:bool}
pub struct Watches<const N:usize>{owner:Identity,items:[Option<Watch>;N]}
impl<const N:usize> Watches<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.items.iter().position(|w|w.is_some_and(|x|x.id==id))
 }
 pub fn observe(&mut self,who:Identity,trusted:bool,id:u32,
 value:i64,tick:u64)->Result<bool,Error>{
  self.auth(who)?;if !trusted||id==0{return Err(Error::Denied);}
  let i=self.find(id).or_else(||self.items.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  let previous=self.items[i].map(|x|x.current).unwrap_or(0);
  let exists=self.items[i].is_some();
  if exists&&tick<self.items[i].unwrap().tick{return Err(Error::Stale);}
  self.items[i]=Some(Watch{id,previous,current:value,tick,has_previous:exists});
  Ok(exists&&previous!=value)
 }
 pub fn evaluate(&self,who:Identity,id:u32,op:arithmetic::Operator,
 number:i64)->Result<i64,Error>{
  self.auth(who)?;
  let v=self.find(id).and_then(|i|self.items[i]).ok_or(Error::Missing)?;
  arithmetic::evaluate(v.current,number,op).map(|x|x.number)
   .map_err(|_|Error::Invalid)
 }
 pub fn changed(&self,who:Identity,id:u32)->Result<bool,Error>{
  self.auth(who)?;
  let v=self.find(id).and_then(|i|self.items[i]).ok_or(Error::Missing)?;
  Ok(v.has_previous&&v.previous!=v.current)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn watched_integer_changes_and_safe_expression_returns_result(){
  let mut w=Watches::<1>::new(id(1));
  w.observe(id(1),true,1,41,1).unwrap();
  assert_eq!(w.observe(id(1),true,1,42,2),Ok(true));
  assert_eq!(w.evaluate(id(1),1,arithmetic::Operator::Add,1),Ok(43));
 }
 #[test]fn recycled_identity_and_untrusted_change_are_denied(){
  let mut w=Watches::<1>::new(id(1));
  assert_eq!(w.observe(id(1),false,1,42,1),Err(Error::Denied));
  assert_eq!(w.changed(id(2),1),Err(Error::Denied));
 }
}