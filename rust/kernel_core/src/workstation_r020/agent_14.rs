//! R020/14 authenticated keyboard shortcuts routed to UI command IDs.
//! No host-side command execution; focus and rights are checked before lookup.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Shortcut{pub owner:u32,pub window:u32,pub key:u32,
 pub modifiers:u8,pub action:u32,pub required_rights:u32,pub enabled:bool}
pub struct Router<const N:usize>{bindings:[Option<Shortcut>;N],focused:[u32;4]}
impl<const N:usize> Router<N>{
 pub const fn new()->Self{Self{bindings:[None;N],focused:[0;4]}}
 pub fn bind(&mut self,owner:u32,window:u32,key:u32,modifiers:u8,
  action:u32,rights:u32)->Result<(),Error>{
  if owner==0||window==0||key==0||action==0||rights==0{return Err(Error::Invalid);}
  if self.bindings.iter().flatten().any(|s|s.owner==owner&&
   s.window==window&&s.key==key&&s.modifiers==modifiers){
   return Err(Error::Conflict);
  }
  let free=self.bindings.iter_mut().find(|s|s.is_none()).ok_or(Error::Full)?;
  *free=Some(Shortcut{owner,window,key,modifiers,action,
   required_rights:rights,enabled:true});Ok(())
 }
 pub fn focus(&mut self,slot:usize,owner:u32,window:u32)->Result<(),Error>{
  if owner==0||window==0||slot>=4{return Err(Error::Invalid);}
  if !self.bindings.iter().flatten().any(|s|s.owner==owner&&s.window==window){
   return Err(Error::Denied);
  }
  self.focused[slot]=window;Ok(())
 }
 pub fn route(&self,owner:u32,slot:usize,key:u32,modifiers:u8,
  live_rights:u32)->Result<u32,Error>{
  if owner==0||slot>=4{return Err(Error::Denied);}
  let focused=self.focused[slot];if focused==0{return Err(Error::Missing);}
  let action=self.bindings.iter().flatten().find(|s|
    s.owner==owner&&s.window==focused&&s.key==key&&
    s.modifiers==modifiers&&s.enabled).ok_or(Error::Missing)?;
  if action.required_rights&live_rights!=action.required_rights{
   return Err(Error::Denied);
  }
  Ok(action.action)
 }
 pub fn unbind_window(&mut self,owner:u32,window:u32)->usize{
  let mut n=0;for b in &mut self.bindings{
   if b.is_some_and(|s|s.owner==owner&&s.window==window){
    *b=None;n+=1;
   }
  }
  if n>0{for f in &mut self.focused{if *f==window{*f=0;}}}
  n
 }
 pub fn count_for(&self,owner:u32)->usize{
  self.bindings.iter().flatten().filter(|s|s.owner==owner).count()
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn keyboard_build_shortcut_uses_focused_owner_window(){
  let mut r=Router::<2>::new();r.bind(7,10,66,1,100,2).unwrap();
  r.focus(0,7,10).unwrap();assert_eq!(r.route(7,0,66,1,2),Ok(100));
 }
 #[test]fn foreign_owner_and_missing_capability_cannot_trigger_build(){
  let mut r=Router::<1>::new();r.bind(7,10,66,1,100,2).unwrap();
  r.focus(0,7,10).unwrap();
  assert_eq!(r.route(8,0,66,1,2),Err(Error::Missing));
  assert_eq!(r.route(7,0,66,1,1),Err(Error::Denied));
 }
 #[test]fn duplicate_shortcut_and_window_teardown_block_old_binding(){
  let mut r=Router::<1>::new();r.bind(7,10,66,1,100,2).unwrap();
  assert_eq!(r.bind(7,10,66,1,101,2),Err(Error::Conflict));
  r.focus(0,7,10).unwrap();assert_eq!(r.unbind_window(7,10),1);
  assert_eq!(r.route(7,0,66,1,2),Err(Error::Missing));
 }
}
