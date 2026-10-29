//! R027/18: bounded desktop/workstation window positioning, split and focus.
//! Geometry is a layout state machine, not a graphical compositor renderer.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:u16,pub y:u16,pub w:u16,pub h:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Window{pub id:u32,pub rect:Rect,pub focused:bool,pub visible:bool}
pub struct Layout<const N:usize>{owner:Identity,width:u16,height:u16,
 windows:[Option<Window>;N]}
impl<const N:usize> Layout<N>{
 pub const fn new(owner:Identity,width:u16,height:u16)->Self{
  Self{owner,width,height,windows:[None;N]}
 }
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.windows.iter().position(|w|w.is_some_and(|w|w.id==id))
 }
 fn valid(&self,rect:Rect)->bool{
  rect.w>0&&rect.h>0&&
  (rect.x as u32+rect.w as u32)<=self.width as u32&&
  (rect.y as u32+rect.h as u32)<=self.height as u32
 }
 pub fn open(&mut self,who:Identity,id:u32,rect:Rect)->Result<(),Error>{
  self.auth(who)?;
  if id==0||!self.valid(rect)||self.slot(id).is_some(){return Err(Error::Invalid);}
  let at=self.windows.iter().position(Option::is_none).ok_or(Error::Full)?;
  for item in &mut self.windows{if let Some(ref mut w)=item{w.focused=false;}}
  self.windows[at]=Some(Window{id,rect,focused:true,visible:true});Ok(())
 }
 pub fn move_resize(&mut self,who:Identity,id:u32,
 rect:Rect)->Result<(),Error>{
  self.auth(who)?;
  if !self.valid(rect){return Err(Error::Invalid);}
  let at=self.slot(id).ok_or(Error::Missing)?;
  let mut w=self.windows[at].ok_or(Error::Missing)?;w.rect=rect;
  self.windows[at]=Some(w);Ok(())
 }
 pub fn focus(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;self.slot(id).ok_or(Error::Missing)?;
  for item in &mut self.windows{
   if let Some(ref mut w)=item{w.focused=w.id==id;}
  }Ok(())
 }
 pub fn get(&self,who:Identity,id:u32)->Result<Window,Error>{
  self.auth(who)?;self.slot(id).and_then(|i|self.windows[i]).ok_or(Error::Missing)
 }
 pub fn close(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let at=self.slot(id).ok_or(Error::Missing)?;
  self.windows[at]=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn actual_two_window_layout_changes_focus_and_geometry(){
  let mut l=Layout::<2>::new(id(1),1920,1080);
  l.open(id(1),1,Rect{x:0,y:0,w:960,h:1080}).unwrap();
  l.open(id(1),2,Rect{x:960,y:0,w:960,h:1080}).unwrap();
  l.focus(id(1),1).unwrap();
  assert!(l.get(id(1),1).unwrap().focused);
  assert!(!l.get(id(1),2).unwrap().focused);
 }
 #[test]fn offscreen_resize_is_rejected_without_moving_window(){
  let mut l=Layout::<1>::new(id(1),800,600);
  let r=Rect{x:0,y:0,w:800,h:600};l.open(id(1),1,r).unwrap();
  assert_eq!(l.move_resize(id(1),1,Rect{x:700,y:0,w:500,h:600}),
   Err(Error::Invalid));assert_eq!(l.get(id(1),1).unwrap().rect,r);
 }
 #[test]fn recycled_user_identity_does_not_control_foreign_window(){
  let mut l=Layout::<1>::new(id(1),800,600);
  l.open(id(1),1,Rect{x:0,y:0,w:400,h:300}).unwrap();
  assert_eq!(l.focus(id(2),1),Err(Error::Denied));
 }
}
