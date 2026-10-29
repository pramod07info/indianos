//! R032/18: concrete multi-pane developer workspace geometry and owner-controlled focus.
use super::Error;
use crate::workstation_r029::agent_18::{Rect,EMPTY};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Pane{pub id:u8,pub rect:Rect,pub active:bool,pub kind:Kind}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Explorer,Editor,Terminal,Diagnostics}
pub struct Workspace{owner:u64,screen:Rect,panes:[Pane;4]}
impl Workspace{
 pub fn new(owner:u64,screen:Rect)->Result<Self,Error>{
  if owner==0||screen.w<640||screen.h<400||
   screen.x as u32+screen.w as u32>u16::MAX as u32||
   screen.y as u32+screen.h as u32>u16::MAX as u32{return Err(Error::Invalid);}
  let mut p=[Pane{id:0,rect:EMPTY,active:false,kind:Kind::Explorer};4];
  let w=screen.w as u32;let h=screen.h as u32;
  let left=(w/5) as u16;let right=(w/4) as u16;let center=screen.w-left-right;
  let upper=(h*3/4) as u16;
  p[0]=Pane{id:1,rect:Rect{x:screen.x,y:screen.y,w:left,h:screen.h},
   active:false,kind:Kind::Explorer};
  p[1]=Pane{id:2,rect:Rect{x:screen.x+left,y:screen.y,w:center,h:upper},
   active:true,kind:Kind::Editor};
  p[2]=Pane{id:3,rect:Rect{x:screen.x+left,y:screen.y+upper,w:center,
   h:screen.h-upper},active:false,kind:Kind::Terminal};
  p[3]=Pane{id:4,rect:Rect{x:screen.x+left+center,y:screen.y,w:right,h:screen.h},
   active:false,kind:Kind::Diagnostics};
  Ok(Self{owner,screen,panes:p})
 }
 pub fn focus(&mut self,owner:u64,id:u8)->Result<(),Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  if !self.panes.iter().any(|p|p.id==id){return Err(Error::Missing);}
  for p in &mut self.panes{p.active=p.id==id;}Ok(())
 }
 pub fn active(&self,owner:u64)->Result<Pane,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  self.panes.iter().copied().find(|p|p.active).ok_or(Error::Missing)
 }
 pub fn complete(&self)->bool{
  let sum:usize=self.panes.iter().map(|p|p.rect.w as usize*p.rect.h as usize).sum();
  sum==self.screen.w as usize*self.screen.h as usize&&
  self.panes.iter().all(|p|p.rect.w>0&&p.rect.h>0)
 }
 pub fn panes(&self,owner:u64)->Result<&[Pane;4],Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  Ok(&self.panes)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn four_workstation_panes_fill_actual_window_without_gaps(){
  let w=Workspace::new(7,Rect{x:10,y:20,w:1200,h:800}).unwrap();
  assert!(w.complete());assert_eq!(w.active(7).unwrap().kind,Kind::Editor);
  assert_eq!(w.panes(7).unwrap().len(),4);
 }
 #[test]fn foreign_session_cannot_steal_editor_focus(){
  let mut w=Workspace::new(7,Rect{x:0,y:0,w:1200,h:800}).unwrap();
  assert_eq!(w.focus(8,3),Err(Error::Denied));
  assert_eq!(w.active(7).unwrap().id,2);
  w.focus(7,3).unwrap();assert_eq!(w.active(7).unwrap().kind,Kind::Terminal);
 }
}