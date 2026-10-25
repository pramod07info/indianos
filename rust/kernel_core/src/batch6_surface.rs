//! R040 tasks 3/9/10: per-process ring-3 surface-damage mailbox policy.
//! No raw framebuffer mapping is given to clients; compositor must validate
//! command sender from trusted syscall ingress and rasterize in userspace.
use crate::batch2_windows::{Windows,Rect};
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,NotOwner,Full,Empty}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Damage{pub owner:Identity,pub window:u32,pub rect:Rect,pub serial:u64}
pub struct Surfaces<const N:usize>{messages:[Option<Damage>;N],count:usize,next:u64}
impl<const N:usize> Surfaces<N>{
 pub const fn new()->Self{Self{messages:[None;N],count:0,next:1}}
 /// All rectangles are local to the compositor's already-owned window.
 pub fn submit<const W:usize>(&mut self,windows:&Windows<W>,
  caller:Identity,window:u32,rect:Rect)->Result<u64,Error>{
  if caller.pid==0||caller.generation==0||window==0||
   !windows.owns(caller.pid,caller.generation,window){return Err(Error::NotOwner);}
  let surface=windows.get(window).ok_or(Error::NotOwner)?.rect;
  if rect.w==0||rect.h==0||rect.x.checked_add(rect.w)
      .is_none_or(|v|v>surface.w)||rect.y.checked_add(rect.h)
      .is_none_or(|v|v>surface.h){return Err(Error::Invalid);}
  if self.count>=N||self.next==u64::MAX{return Err(Error::Full);}
  let serial=self.next;self.next+=1;
  self.messages[self.count]=Some(Damage{owner:caller,window,rect,serial});
  self.count+=1;Ok(serial)
 }
 /// Trusted compositor only: caller is the scheduler-authenticated service.
 pub fn next(&mut self)->Result<Damage,Error>{
  if self.count==0{return Err(Error::Empty);}
  let event=self.messages[0].ok_or(Error::Empty)?;
  for i in 1..self.count{self.messages[i-1]=self.messages[i];}
  self.count-=1;self.messages[self.count]=None;Ok(event)
 }
 pub fn retire(&mut self,caller:Identity){
  let mut i=0;
  while i<self.count{
   if self.messages[i].is_some_and(|d|d.owner==caller){
    for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
    self.count-=1;self.messages[self.count]=None;
   }else{i+=1;}
  }
 }
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch2_windows::Window;
 fn w()->Windows<2>{
  let mut x=Windows::new();
  x.create(Window{pid:2,generation:4,id:7,
   rect:Rect{x:0,y:0,w:100,h:80}}).unwrap();x
 }
 #[test]fn only_owner_may_submit_bounded_damage(){
  let windows=w();let mut q=Surfaces::<1>::new();
  let id=Identity{pid:2,generation:4};
  assert_eq!(q.submit(&windows,Identity{pid:3,..id},7,
   Rect{x:0,y:0,w:10,h:10}),Err(Error::NotOwner));
  assert_eq!(q.submit(&windows,id,7,Rect{x:80,y:0,w:30,h:2}),
   Err(Error::Invalid));
  q.submit(&windows,id,7,Rect{x:1,y:2,w:10,h:10}).unwrap();
  assert_eq!(q.submit(&windows,id,7,Rect{x:0,y:0,w:1,h:1}),Err(Error::Full));
  assert_eq!(q.next().unwrap().owner,id);
  assert_eq!(q.next(),Err(Error::Empty));
 }
 #[test]fn retirement_drops_queued_frame_updates(){
  let windows=w();let mut q=Surfaces::<1>::new();let id=Identity{pid:2,generation:4};
  q.submit(&windows,id,7,Rect{x:1,y:1,w:2,h:2}).unwrap();
  q.retire(id);assert_eq!(q.next(),Err(Error::Empty));
 }
}