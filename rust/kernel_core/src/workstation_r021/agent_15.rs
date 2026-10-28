//! R021/15: owner-aware focus history and modal window policy on real Windows.
//! Dispatch is metadata only; compositor/keyboard driver must honor this policy.
use super::Error;
use crate::batch2_windows::Windows;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Focus{pub window:u32,pub previous:u32,pub modal:u32,
 pub revision:u64}
pub struct Session{owner:Identity,state:Focus}
impl Session{
 pub const fn new(owner:Identity)->Self{
  Self{owner,state:Focus{window:0,previous:0,modal:0,revision:0}}
 }
 fn check<const N:usize>(&self,caller:Identity,windows:&Windows<N>,
   window:u32)->Result<(),Error>{
  if caller.pid==0||caller!=self.owner||
   !windows.owns(caller.pid,caller.generation,window){
   return Err(Error::Denied);
  }Ok(())
 }
 pub fn focus<const N:usize>(&mut self,caller:Identity,windows:&Windows<N>,
  target:u32,expected:u64)->Result<u64,Error>{
  self.check(caller,windows,target)?;
  if self.state.revision!=expected{return Err(Error::Stale);}
  if self.state.modal!=0&&self.state.modal!=target{return Err(Error::Denied);}
  let next=expected.checked_add(1).ok_or(Error::Overflow)?;
  self.state.previous=self.state.window;self.state.window=target;
  self.state.revision=next;Ok(next)
 }
 pub fn set_modal<const N:usize>(&mut self,caller:Identity,windows:&Windows<N>,
  modal:u32,expected:u64)->Result<u64,Error>{
  self.check(caller,windows,modal)?;
  if expected!=self.state.revision{return Err(Error::Stale);}
  if self.state.modal!=0{return Err(Error::Conflict);}
  let next=expected.checked_add(1).ok_or(Error::Overflow)?;
  self.state.modal=modal;self.state.previous=self.state.window;
  self.state.window=modal;self.state.revision=next;Ok(next)
 }
 pub fn release_modal(&mut self,caller:Identity,modal:u32,
  expected:u64)->Result<u64,Error>{
  if caller!=self.owner||caller.pid==0{return Err(Error::Denied);}
  if self.state.modal!=modal||modal==0||expected!=self.state.revision{
   return Err(Error::Stale);
  }
  let next=expected.checked_add(1).ok_or(Error::Overflow)?;
  self.state.modal=0;self.state.window=self.state.previous;
  self.state.previous=0;self.state.revision=next;Ok(next)
 }
 pub fn retire_window(&mut self,caller:Identity,window:u32)->Result<(),Error>{
  if caller.pid==0||caller!=self.owner{return Err(Error::Denied);}
  if self.state.modal==window{self.state.modal=0;}
  if self.state.window==window{self.state.window=0;}
  if self.state.previous==window{self.state.previous=0;}Ok(())
 }
 pub fn current(&self,caller:Identity)->Result<Focus,Error>{
  if caller.pid==0||caller!=self.owner{Err(Error::Denied)}
  else{Ok(self.state)}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch2_windows::{Window,Rect};
 fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 fn window(pid:u32,id:u32)->Window{
  Window{pid,generation:1,id,rect:Rect{x:1,y:1,w:100,h:100}}
 }
 #[test]fn modal_dialog_blocks_editor_focus_until_closed(){
  let mut w=Windows::<2>::new();w.create(window(7,1)).unwrap();
  w.create(window(7,2)).unwrap();
  let mut s=Session::new(id(1));
  s.focus(id(1),&w,1,0).unwrap();
  s.set_modal(id(1),&w,2,1).unwrap();
  assert_eq!(s.focus(id(1),&w,1,2),Err(Error::Denied));
  s.release_modal(id(1),2,2).unwrap();
  assert_eq!(s.current(id(1)).unwrap().window,1);
 }
 #[test]fn foreign_or_recycled_pid_cannot_hijack_window_focus(){
  let mut w=Windows::<1>::new();w.create(window(7,1)).unwrap();
  let mut s=Session::new(id(1));
  assert_eq!(s.focus(id(2),&w,1,0),Err(Error::Denied));
 }
 #[test]fn stale_focus_revision_does_not_reorder_windows(){
  let mut w=Windows::<1>::new();w.create(window(7,1)).unwrap();
  let mut s=Session::new(id(1));s.focus(id(1),&w,1,0).unwrap();
  assert_eq!(s.focus(id(1),&w,1,0),Err(Error::Stale));
  assert_eq!(s.current(id(1)).unwrap().revision,1);
 }
}
