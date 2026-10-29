//! R028/18: owner-scoped keyboard shortcuts with collision-free command routing.
//! This is a stateful input routing layer, not a compositor.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{Open,Save,Search,Terminal,Build,Debug}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Binding{pub key:u16,pub modifiers:u8,pub action:Action}
pub struct Bindings<const N:usize>{owner:Identity,items:[Option<Binding>;N]}
impl<const N:usize> Bindings<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn bind(&mut self,who:Identity,key:u16,mods:u8,
 action:Action)->Result<(),Error>{
  self.auth(who)?;
  if key==0||mods&!0x0f!=0{return Err(Error::Invalid);}
  if self.items.iter().flatten().any(|b|b.key==key&&b.modifiers==mods){
   return Err(Error::Conflict);
  }
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.items[i]=Some(Binding{key,modifiers:mods,action});Ok(())
 }
 pub fn resolve(&self,who:Identity,key:u16,mods:u8)
  ->Result<Option<Action>,Error>{
  self.auth(who)?;
  Ok(self.items.iter().flatten().find(|b|b.key==key&&b.modifiers==mods)
   .map(|b|b.action))
 }
 pub fn unbind(&mut self,who:Identity,key:u16,mods:u8)->Result<(),Error>{
  self.auth(who)?;
  let i=self.items.iter().position(|x|x.is_some_and(|b|
   b.key==key&&b.modifiers==mods)).ok_or(Error::Missing)?;
  self.items[i]=None;Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn save_and_terminal_hotkeys_route_distinct_commands(){
  let mut b=Bindings::<2>::new(id(1));
  b.bind(id(1),83,1,Action::Save).unwrap();
  b.bind(id(1),84,1,Action::Terminal).unwrap();
  assert_eq!(b.resolve(id(1),83,1),Ok(Some(Action::Save)));
 }
 #[test]fn duplicate_binding_and_foreign_process_are_rejected(){
  let mut b=Bindings::<1>::new(id(1));
  b.bind(id(1),83,1,Action::Save).unwrap();
  assert_eq!(b.bind(id(1),83,1,Action::Open),Err(Error::Conflict));
  assert_eq!(b.resolve(id(2),83,1),Err(Error::Denied));
 }
}