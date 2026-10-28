//! R020/10 verified debugger watchpoints over trusted register snapshots.
//! No eval of source expressions, no target memory access and no live CPU traps.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Watch{pub owner:u32,pub session:u32,pub register:u8,
 pub compare:i64,pub last:i64,pub triggers:u32,pub enabled:bool}
pub struct Watches<const N:usize>{slots:[Option<Watch>;N]}
impl<const N:usize> Watches<N>{
 pub const fn new()->Self{Self{slots:[None;N]}}
 fn find(&self,owner:u32,session:u32,register:u8)->Option<usize>{
  self.slots.iter().position(|w|w.is_some_and(|w|
    w.owner==owner&&w.session==session&&w.register==register))
 }
 pub fn add(&mut self,owner:u32,session:u32,register:u8,
  threshold:i64)->Result<(),Error>{
  if owner==0||session==0||register>31{return Err(Error::Invalid);}
  if self.find(owner,session,register).is_some(){return Err(Error::Conflict);}
  let free=self.slots.iter_mut().find(|w|w.is_none()).ok_or(Error::Full)?;
  *free=Some(Watch{owner,session,register,compare:threshold,last:0,
   triggers:0,enabled:true});Ok(())
 }
 pub fn observe(&mut self,supervisor:bool,owner:u32,session:u32,
  register:u8,value:i64)->Result<bool,Error>{
  if !supervisor{return Err(Error::Denied);}
  let i=self.find(owner,session,register).ok_or(Error::Missing)?;
  let mut w=self.slots[i].ok_or(Error::Missing)?;
  if !w.enabled{return Ok(false);}
  let triggered=value>=w.compare&&w.last<w.compare;
  if triggered{w.triggers=w.triggers.checked_add(1).ok_or(Error::Overflow)?;}
  w.last=value;self.slots[i]=Some(w);Ok(triggered)
 }
 pub fn enable(&mut self,owner:u32,session:u32,register:u8,on:bool)
  ->Result<(),Error>{
  let i=self.find(owner,session,register).ok_or(Error::Missing)?;
  let mut w=self.slots[i].ok_or(Error::Missing)?;
  w.enabled=on;self.slots[i]=Some(w);Ok(())
 }
 pub fn revoke_session(&mut self,owner:u32,session:u32)->usize{
  let mut n=0;for w in &mut self.slots{
   if w.is_some_and(|x|x.owner==owner&&x.session==session){*w=None;n+=1;}
  }n
 }
 pub fn triggered_count(&self,owner:u32,session:u32)->usize{
  self.slots.iter().flatten().filter(|w|w.owner==owner&&w.session==session&&
    w.triggers>0).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn threshold_triggers_only_on_low_to_high_transition(){
  let mut d=Watches::<1>::new();d.add(7,1,0,10).unwrap();
  assert_eq!(d.observe(true,7,1,0,9),Ok(false));
  assert_eq!(d.observe(true,7,1,0,10),Ok(true));
  assert_eq!(d.observe(true,7,1,0,11),Ok(false));
  assert_eq!(d.triggered_count(7,1),1);
 }
 #[test]fn untrusted_observation_and_wrong_owner_cannot_change_register(){
  let mut d=Watches::<1>::new();d.add(7,1,0,10).unwrap();
  assert_eq!(d.observe(false,7,1,0,20),Err(Error::Denied));
  assert_eq!(d.observe(true,8,1,0,20),Err(Error::Missing));
  assert_eq!(d.observe(true,7,1,0,10),Ok(true));
 }
 #[test]fn revoke_debugging_session_discards_watchpoints(){
  let mut d=Watches::<2>::new();d.add(7,1,0,10).unwrap();d.add(7,2,0,1).unwrap();
  assert_eq!(d.revoke_session(7,1),1);
  assert_eq!(d.observe(true,7,1,0,10),Err(Error::Missing));
  assert_eq!(d.observe(true,7,2,0,2),Ok(true));
 }
}
