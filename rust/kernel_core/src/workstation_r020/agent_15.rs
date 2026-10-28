//! R020/15 live window geometry for developer editor, terminal, diagnostics.
//! Uses existing kernel-authenticated window table, not a mock UI renderer.
use super::Error;
use crate::batch2_windows::{Windows,Rect,WIDTH,HEIGHT};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Layout{EditorOnly,SplitEditorTerminal,ThreePane}
pub fn apply<const N:usize>(windows:&mut Windows<N>,owner:u32,generation:u64,
 editor:u32,terminal:u32,diagnostics:u32,layout:Layout)->Result<usize,Error>{
 if owner==0||generation==0{return Err(Error::Denied);}
 let half=WIDTH/2;
 match layout{
  Layout::EditorOnly=>{
   windows.move_batch(owner,generation,&[(editor,
    Rect{x:0,y:0,w:WIDTH,h:HEIGHT})])
    .map_err(|_|Error::Denied)
  }
  Layout::SplitEditorTerminal=>{
   windows.tile_two(owner,generation,editor,terminal)
    .map_err(|_|Error::Denied)?;Ok(2)
  }
  Layout::ThreePane=>{
   let top=HEIGHT*2/3;
   windows.move_batch(owner,generation,&[
    (editor,Rect{x:0,y:0,w:half,h:top}),
    (terminal,Rect{x:half,y:0,w:WIDTH-half,h:top}),
    (diagnostics,Rect{x:0,y:top,w:WIDTH,h:HEIGHT-top})
   ]).map_err(|_|Error::Denied)
  }
 }
}
pub fn read_layout<const N:usize>(windows:&Windows<N>,owner:u32,generation:u64,
 id:u32)->Result<Rect,Error>{
 if !windows.owns(owner,generation,id){return Err(Error::Denied);}
 Ok(windows.get(id).ok_or(Error::Missing)?.rect)
}
pub fn owner_can_tile<const N:usize>(w:&Windows<N>,owner:u32,
 generation:u64,ids:&[u32])->bool{
 owner!=0&&generation!=0&&ids.iter().all(|id|w.owns(owner,generation,*id))
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch2_windows::Window;
 fn window(id:u32,owner:u32)->Window{Window{
  pid:owner,generation:1,id,rect:Rect{x:1,y:1,w:100,h:100}}}
 #[test]fn three_pane_real_kernel_window_geometry_is_applied(){
  let mut w=Windows::<3>::new();
  for id in 1..=3{w.create(window(id,7)).unwrap();}
  assert_eq!(apply(&mut w,7,1,1,2,3,Layout::ThreePane),Ok(3));
  assert_eq!(read_layout(&w,7,1,3).unwrap().w,WIDTH);
  assert_eq!(read_layout(&w,7,1,2).unwrap().x,WIDTH/2);
 }
 #[test]fn foreign_debugger_window_blocks_all_atomic_tiling(){
  let mut w=Windows::<3>::new();
  w.create(window(1,7)).unwrap();w.create(window(2,7)).unwrap();
  w.create(window(3,8)).unwrap();
  assert_eq!(apply(&mut w,7,1,1,2,3,Layout::ThreePane),Err(Error::Denied));
  assert_eq!(read_layout(&w,7,1,1).unwrap().x,1);
 }
 #[test]fn stale_window_generation_cannot_rearrange_workspace(){
  let mut w=Windows::<1>::new();w.create(window(1,7)).unwrap();
  assert_eq!(apply(&mut w,7,2,1,2,3,Layout::EditorOnly),Err(Error::Denied));
  assert!(!owner_can_tile(&w,7,2,&[1]));
 }
}
