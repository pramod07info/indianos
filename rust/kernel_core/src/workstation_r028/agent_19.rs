//! R028/19: bounded developer-workstation appearance and interaction profiles.
//! Profiles contain typed layout preferences, not rendered GUI themes.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Theme{Light,Dark,Crystal}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Profile{pub id:u32,pub theme:Theme,pub font_scale:u8,
 pub split_ratio:u8,pub animations:bool}
pub struct Profiles<const N:usize>{owner:Identity,items:[Option<Profile>;N],active:u32}
impl<const N:usize> Profiles<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N],active:0}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn find(&self,id:u32)->Option<usize>{
  self.items.iter().position(|x|x.is_some_and(|p|p.id==id))
 }
 pub fn save(&mut self,who:Identity,p:Profile)->Result<(),Error>{
  self.auth(who)?;
  if p.id==0||p.font_scale<75||p.font_scale>200||
   p.split_ratio<20||p.split_ratio>80{return Err(Error::Invalid);}
  let i=self.find(p.id).or_else(||self.items.iter().position(Option::is_none))
   .ok_or(Error::Full)?;
  self.items[i]=Some(p);if self.active==0{self.active=p.id;}Ok(())
 }
 pub fn switch(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;
  if self.find(id).is_none(){return Err(Error::Missing);}
  self.active=id;Ok(())
 }
 pub fn active(&self,who:Identity)->Result<Profile,Error>{
  self.auth(who)?;
  self.find(self.active).and_then(|i|self.items[i]).ok_or(Error::Missing)
 }
 pub fn delete(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let i=self.find(id).ok_or(Error::Missing)?;
  self.items[i]=None;if self.active==id{
   self.active=self.items.iter().flatten().next().map(|x|x.id).unwrap_or(0);
  }Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn developer_switches_light_and_crystal_workstation_preferences(){
  let mut p=Profiles::<2>::new(id(1));
  p.save(id(1),Profile{id:1,theme:Theme::Light,font_scale:100,
   split_ratio:50,animations:true}).unwrap();
  p.save(id(1),Profile{id:2,theme:Theme::Crystal,font_scale:125,
   split_ratio:60,animations:false}).unwrap();
  p.switch(id(1),2).unwrap();assert_eq!(p.active(id(1)).unwrap().theme,Theme::Crystal);
 }
 #[test]fn invalid_split_and_foreign_session_rejected(){
  let mut p=Profiles::<1>::new(id(1));
  assert_eq!(p.save(id(1),Profile{id:1,theme:Theme::Dark,
   font_scale:100,split_ratio:99,animations:true}),Err(Error::Invalid));
  assert_eq!(p.active(id(2)),Err(Error::Denied));
 }
}