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
 /// Tolerant client dirty-rectangle submission with strict clipping to
 /// the current live owned window. Completely off-surface submissions fail.
 /// Returned damage can never reveal pixels outside the sender's window.
 pub fn submit_clipped<const W:usize>(&mut self,windows:&Windows<W>,
  caller:Identity,window:u32,rect:Rect)->Result<u64,Error>{
  if caller.pid==0||caller.generation==0||window==0||
    !windows.owns(caller.pid,caller.generation,window){
   return Err(Error::NotOwner);
  }
  let window_rect=windows.get(window).ok_or(Error::NotOwner)?.rect;
  if rect.w==0||rect.h==0||rect.x>=window_rect.w||rect.y>=window_rect.h{
   return Err(Error::Invalid);
  }
  let right=rect.x.checked_add(rect.w).unwrap_or(u32::MAX)
    .min(window_rect.w);
  let bottom=rect.y.checked_add(rect.h).unwrap_or(u32::MAX)
    .min(window_rect.h);
  self.submit(windows,caller,window,Rect{
    x:rect.x,y:rect.y,w:right-rect.x,h:bottom-rect.y,
  })
 }
 /// Trusted compositor only: caller is the scheduler-authenticated service.
 pub fn next(&mut self)->Result<Damage,Error>{
  if self.count==0{return Err(Error::Empty);}
  let event=self.messages[0].ok_or(Error::Empty)?;
  for i in 1..self.count{self.messages[i-1]=self.messages[i];}
  self.count-=1;self.messages[self.count]=None;Ok(event)
 }
 /// Compositor-only drain: translate a client-local dirty rectangle into
 /// validated screen-space coordinates using the CURRENT window geometry.
 /// Ignore obsolete damage from retired processes, recycled window IDs or
 /// windows resized after the damage was queued. A ring-3 compositor must
 /// receive these results over an authenticated kernel-owned IPC channel.
 pub fn next_live<const W:usize>(&mut self,windows:&Windows<W>)
  ->Result<(Damage,Rect),Error>{
  loop{
   let event=self.next()?;
   if !windows.owns(event.owner.pid,event.owner.generation,event.window){
    continue;
   }
   let Some(window)=windows.get(event.window)else{continue;};
   let local=event.rect;
   if local.x.checked_add(local.w).is_none_or(|x|x>window.rect.w)||
      local.y.checked_add(local.h).is_none_or(|y|y>window.rect.h){
    continue;
   }
   let Some(x)=window.rect.x.checked_add(local.x)else{continue;};
   let Some(y)=window.rect.y.checked_add(local.y)else{continue;};
   let Some(right)=x.checked_add(local.w)else{continue;};
   let Some(bottom)=y.checked_add(local.h)else{continue;};
   if right>crate::batch2_windows::WIDTH||
      bottom>crate::batch2_windows::HEIGHT{continue;}
   return Ok((event,Rect{x,y,w:local.w,h:local.h}));
  }
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
 #[test]fn compositor_drains_authenticated_local_damage_in_screen_space(){
  let mut windows=Windows::<2>::new();
  let owner=Identity{pid:2,generation:4};
  windows.create(Window{pid:2,generation:4,id:7,
    rect:Rect{x:45,y:65,w:100,h:80}}).unwrap();
  let mut q=Surfaces::<3>::new();
  q.submit(&windows,owner,7,Rect{x:5,y:6,w:10,h:12}).unwrap();
  let (event,rect)=q.next_live(&windows).unwrap();
  assert_eq!(event.owner,owner);
  assert_eq!(rect,Rect{x:50,y:71,w:10,h:12});
  assert_eq!(q.next_live(&windows),Err(Error::Empty));
 }
 #[test]fn compositor_discards_stale_generation_and_resized_damage(){
  let mut windows=Windows::<2>::new();
  let old=Identity{pid:2,generation:4};
  windows.create(Window{pid:2,generation:4,id:7,
    rect:Rect{x:0,y:0,w:100,h:80}}).unwrap();
  let mut q=Surfaces::<4>::new();
  q.submit(&windows,old,7,Rect{x:90,y:5,w:10,h:10}).unwrap();
  windows.move_to(2,4,7,Rect{x:1,y:1,w:40,h:40}).unwrap();
  assert_eq!(q.next_live(&windows),Err(Error::Empty));
  q.submit(&windows,old,7,Rect{x:1,y:1,w:5,h:5}).unwrap();
  windows.retire(2,4);
  windows.create(Window{pid:2,generation:5,id:7,
    rect:Rect{x:0,y:0,w:100,h:80}}).unwrap();
  assert_eq!(q.next_live(&windows),Err(Error::Empty));
 }
 #[test]fn retirement_drops_queued_frame_updates(){
  let windows=w();let mut q=Surfaces::<1>::new();let id=Identity{pid:2,generation:4};
  q.submit(&windows,id,7,Rect{x:1,y:1,w:2,h:2}).unwrap();
  q.retire(id);assert_eq!(q.next(),Err(Error::Empty));
 }

 #[test]fn compositor_clips_oversized_user_rectangles_to_current_surface(){
  let mut win=w();let owner=Identity{pid:2,generation:4};
  let mut mailbox=Surfaces::<2>::new();
  mailbox.submit_clipped(&win,owner,7,
    Rect{x:95,y:70,w:30,h:40}).unwrap();
  let (_,frame)=mailbox.next_live(&win).unwrap();
  assert_eq!(frame,Rect{x:95,y:70,w:5,h:10});
  assert_eq!(mailbox.submit_clipped(&win,owner,7,
    Rect{x:101,y:0,w:20,h:10}),Err(Error::Invalid));
  assert_eq!(mailbox.submit_clipped(&win,owner,7,
    Rect{x:0,y:0,w:0,h:10}),Err(Error::Invalid));
  win.retire(2,4);
  assert_eq!(mailbox.submit_clipped(&win,owner,7,
    Rect{x:0,y:0,w:1,h:1}),Err(Error::NotOwner));
 }
 #[test]fn recycled_pid_cannot_publish_clipped_damage(){
  let win=w();let owner=Identity{pid:2,generation:4};
  let mut mailbox=Surfaces::<2>::new();
  assert_eq!(mailbox.submit_clipped(&win,Identity{generation:5,..owner},
    7,Rect{x:0,y:0,w:10,h:10}),Err(Error::NotOwner));
  assert_eq!(mailbox.next(),Err(Error::Empty));
 }

}