//! R031/19: pixel-exact editor/terminal split panes with minimum usable areas.
use super::Error;
use crate::workstation_r029::agent_18::{Rect,EMPTY};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Axis{Horizontal,Vertical}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Layout{pub editor:Rect,pub secondary:Rect,pub ratio:u8,pub axis:Axis}
pub fn split(screen:Rect,axis:Axis,ratio:u8,min:u16)->Result<Layout,Error>{
 if ratio<10||ratio>90||min==0||screen.w==0||screen.h==0{
  return Err(Error::Invalid);
 }
 let length=if axis==Axis::Horizontal{screen.w}else{screen.h};
 if length as u32<2*min as u32{return Err(Error::Full);}
 let first=(length as u32*ratio as u32/100) as u16;
 if first<min||length-first<min{return Err(Error::Full);}
 if screen.x as u32+screen.w as u32>u16::MAX as u32||
  screen.y as u32+screen.h as u32>u16::MAX as u32{return Err(Error::Overflow);}
 let (editor,secondary)=match axis{
  Axis::Horizontal=>(Rect{x:screen.x,y:screen.y,w:first,h:screen.h},
   Rect{x:screen.x+first,y:screen.y,w:length-first,h:screen.h}),
  Axis::Vertical=>(Rect{x:screen.x,y:screen.y,w:screen.w,h:first},
   Rect{x:screen.x,y:screen.y+first,w:screen.w,h:length-first})
 };
 Ok(Layout{editor,secondary,ratio,axis})
}
pub fn resize(layout:Layout,screen:Rect,ratio:u8,min:u16)
 ->Result<Layout,Error>{split(screen,layout.axis,ratio,min)}
pub fn valid(layout:Layout,screen:Rect)->bool{
 layout.editor.w as u32*layout.editor.h as u32+
 layout.secondary.w as u32*layout.secondary.h as u32==
 screen.w as u32*screen.h as u32
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn dragged_editor_split_preserves_entire_viewport_no_overlap(){
  let screen=Rect{x:10,y:20,w:1200,h:800};
  let first=split(screen,Axis::Horizontal,65,200).unwrap();
  let second=resize(first,screen,55,200).unwrap();
  assert!(valid(second,screen));assert_eq!(second.editor.w,660);
 }
 #[test]fn tiny_pane_rejected_instead_of_becoming_unusable(){
  let screen=Rect{x:0,y:0,w:400,h:400};
  assert_eq!(split(screen,Axis::Vertical,10,100),Err(Error::Full));
  assert_eq!(split(EMPTY,Axis::Horizontal,50,100),Err(Error::Invalid));
 }
}