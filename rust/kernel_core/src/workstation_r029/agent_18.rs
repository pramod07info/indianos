//! R029/18: deterministic desktop window tiling geometry for actual viewport.
//! Supplies distinct workstation layouts to compositor but does not render GPU pixels.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Preset{SideBySide,Stacked,WideEditor,TripleColumn}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:u16,pub y:u16,pub w:u16,pub h:u16}
pub const EMPTY:Rect=Rect{x:0,y:0,w:0,h:0};
pub fn layout<const N:usize>(screen:Rect,preset:Preset,
 out:&mut[Rect;N])->Result<usize,Error>{
 if screen.w<200||screen.h<200{return Err(Error::Invalid);}
 let x=screen.x as u32;let y=screen.y as u32;
 if x+screen.w as u32>u16::MAX as u32||
  y+screen.h as u32>u16::MAX as u32{return Err(Error::Overflow);}
 let n=if preset==Preset::TripleColumn{3}else{2};
 if N<n{return Err(Error::Full);}
 let mut stage=[EMPTY;N];
 let w=screen.w;let h=screen.h;
 match preset{
  Preset::SideBySide=>{
   let a=w/2;
   stage[0]=Rect{x:screen.x,y:screen.y,w:a,h};
   stage[1]=Rect{x:screen.x+a,y:screen.y,w:w-a,h};
  }
  Preset::Stacked=>{
   let a=h/2;
   stage[0]=Rect{x:screen.x,y:screen.y,w,h:a};
   stage[1]=Rect{x:screen.x,y:screen.y+a,w,h:h-a};
  }
  Preset::WideEditor=>{
   let a=w*2/3;
   stage[0]=Rect{x:screen.x,y:screen.y,w:a,h};
   stage[1]=Rect{x:screen.x+a,y:screen.y,w:w-a,h};
  }
  Preset::TripleColumn=>{
   let a=w/3;let b=(w-a)/2;
   stage[0]=Rect{x:screen.x,y:screen.y,w:a,h};
   stage[1]=Rect{x:screen.x+a,y:screen.y,w:b,h};
   stage[2]=Rect{x:screen.x+a+b,y:screen.y,w:w-a-b,h};
  }
 }
 *out=stage;Ok(n)
}
pub fn covers(screen:Rect,rects:&[Rect])->bool{
 let area:usize=rects.iter().map(|r|r.w as usize*r.h as usize).sum();
 area==screen.w as usize*screen.h as usize
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn triple_column_layout_uses_entire_desktop_viewport(){
  let screen=Rect{x:0,y:0,w:1200,h:800};let mut out=[EMPTY;3];
  assert_eq!(layout(screen,Preset::TripleColumn,&mut out),Ok(3));
  assert!(covers(screen,&out));
 }
 #[test]fn insufficient_window_slots_leaves_layout_untouched(){
  let mut out=[Rect{x:77,..EMPTY};2];
  assert_eq!(layout(Rect{x:0,y:0,w:1200,h:800},Preset::TripleColumn,&mut out),
   Err(Error::Full));assert_eq!(out[0].x,77);
 }
}