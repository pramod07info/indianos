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
 /// Merge repeated client updates into one queued full-window invalidation
 /// when the queue is already saturated. Preserves event ordering and never
 /// aliases another process's window, generation or compositor privileges.
 pub fn submit_coalesced<const W:usize>(&mut self,windows:&Windows<W>,
  caller:Identity,window:u32,rect:Rect)->Result<u64,Error>{
  if caller.pid==0||caller.generation==0||
    !windows.owns(caller.pid,caller.generation,window){
   return Err(Error::NotOwner);
  }
  let geometry=windows.get(window).ok_or(Error::NotOwner)?.rect;
  if rect.w==0||rect.h==0||
     rect.x.checked_add(rect.w).is_none_or(|end|end>geometry.w)||
     rect.y.checked_add(rect.h).is_none_or(|end|end>geometry.h){
    return Err(Error::Invalid);
  }
  for old in self.messages[..self.count].iter_mut().flatten(){
   if old.owner==caller&&old.window==window{
    let x=old.rect.x.min(rect.x);let y=old.rect.y.min(rect.y);
    let right=(old.rect.x+old.rect.w).max(rect.x+rect.w);
    let bottom=(old.rect.y+old.rect.h).max(rect.y+rect.h);
    old.rect=Rect{x,y,w:right-x,h:bottom-y};
    return Ok(old.serial);
   }
  }
  self.submit(windows,caller,window,rect)
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


 #[test]fn compositor_damage_coalescing_merges_before_queue_overflow(){
  let win=w();let owner=Identity{pid:2,generation:4};
  let mut q=Surfaces::<1>::new();
  let first=q.submit_coalesced(&win,owner,7,Rect{x:1,y:1,w:2,h:2}).unwrap();
  for i in 2..50{
   assert_eq!(q.submit_coalesced(&win,owner,7,
      Rect{x:i,y:2,w:1,h:1}),Ok(first));
  }
  let next=q.next().unwrap();
  assert_eq!(next.rect,Rect{x:1,y:1,w:49,h:2});
  assert_eq!(q.next(),Err(Error::Empty));
 }
 #[test]fn compositor_coalescing_rejects_wrong_pid_generation_and_invalid_rect(){
  let win=w();let owner=Identity{pid:2,generation:4};
  let mut q=Surfaces::<1>::new();
  q.submit_coalesced(&win,owner,7,Rect{x:0,y:0,w:2,h:2}).unwrap();
  assert_eq!(q.submit_coalesced(&win,Identity{generation:5,..owner},7,
     Rect{x:0,y:0,w:2,h:2}),Err(Error::NotOwner));
  assert_eq!(q.submit_coalesced(&win,owner,7,
     Rect{x:90,y:0,w:20,h:2}),Err(Error::Invalid));
  assert_eq!(q.next().unwrap().rect,Rect{x:0,y:0,w:2,h:2});
 }

}

/// R006 compositor transition: atomic frame batches, full-window redraw,
/// scoped cancellation and backpressure introspection. These act on the
/// existing authenticated mailbox, not arbitrary framebuffer pointers.
impl<const N:usize> Surfaces<N>{
 pub fn submit_batch<const W:usize>(&mut self,windows:&Windows<W>,
  caller:Identity,updates:&[(u32,Rect)])->Result<usize,Error>{
  if updates.is_empty()||updates.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
  for &(window,rect) in updates{staged.submit(windows,caller,window,rect)?;}
  *self=staged;Ok(updates.len())
 }
 pub fn redraw_window<const W:usize>(&mut self,windows:&Windows<W>,
  caller:Identity,window:u32)->Result<u64,Error>{
  if !windows.owns(caller.pid,caller.generation,window){
   return Err(Error::NotOwner);
  }
  let geometry=windows.get(window).ok_or(Error::NotOwner)?.rect;
  self.submit_coalesced(windows,caller,window,Rect{
   x:0,y:0,w:geometry.w,h:geometry.h,
  })
 }
 pub fn pending_owner(&self,caller:Identity)->usize{
  if caller.pid==0||caller.generation==0{return 0;}
  self.messages[..self.count].iter().flatten()
   .filter(|m|m.owner==caller).count()
 }
 /// Cancel stale frame publications after an app window is closed without
 /// discarding other windows or other owners' pending work.
 pub fn cancel_window(&mut self,caller:Identity,window:u32)->usize{
  if caller.pid==0||caller.generation==0||window==0{return 0;}
  let mut removed=0;let mut i=0;
  while i<self.count{
   if self.messages[i].is_some_and(|d|d.owner==caller&&d.window==window){
    for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
    self.count-=1;self.messages[self.count]=None;removed+=1;
   }else{i+=1;}
  }
  removed
 }
 /// Render only after compositor validates a fresh grant, then retire
 /// damage for one owner without touching any neighboring process.
 pub fn drain_owner(&mut self,owner:Identity,output:&mut[Damage])
  ->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::NotOwner);}
  let needed=self.pending_owner(owner);
  if output.len()<needed{return Err(Error::Full);}
  let mut count=0;let mut i=0;
  while i<self.count{
   if self.messages[i].is_some_and(|m|m.owner==owner){
    output[count]=self.messages[i].ok_or(Error::Empty)?;
    count+=1;
    for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
    self.count-=1;self.messages[self.count]=None;
   }else{i+=1;}
  }
  Ok(count)
 }
}
#[cfg(test)]
mod r006_compositor_batch_tests{
 use super::*;
 use crate::batch2_windows::Window;
 fn owner()->Identity{Identity{pid:20,generation:7}}
 fn windows()->Windows<3>{
  let mut x=Windows::new();
  x.create(Window{pid:20,generation:7,id:1,rect:Rect{x:0,y:0,w:100,h:90}}).unwrap();
  x.create(Window{pid:20,generation:7,id:2,rect:Rect{x:150,y:20,w:40,h:50}}).unwrap();
  x.create(Window{pid:30,generation:7,id:3,rect:Rect{x:250,y:20,w:40,h:50}}).unwrap();
  x
 }
 #[test]fn multi_window_damage_is_fully_atomic_even_when_invalid(){
  let w=windows();let mut q=Surfaces::<4>::new();
  let r=Rect{x:1,y:2,w:5,h:6};
  assert_eq!(q.submit_batch(&w,owner(),&[(1,r),(3,r)]),Err(Error::NotOwner));
  assert_eq!(q.pending_owner(owner()),0);
  assert_eq!(q.submit_batch(&w,owner(),&[(1,r),(2,r)]),Ok(2));
  assert_eq!(q.pending_owner(owner()),2);
  assert_eq!(q.submit_batch(&w,owner(),&[(1,r),(2,Rect{x:99,..r})]),
   Err(Error::Invalid));
  assert_eq!(q.pending_owner(owner()),2);
 }
 #[test]fn full_redraw_and_owner_drain_do_not_drop_peers(){
  let w=windows();let mut q=Surfaces::<3>::new();
  let peer=Identity{pid:30,generation:7};
  q.redraw_window(&w,owner(),1).unwrap();
  q.redraw_window(&w,peer,3).unwrap();
  q.redraw_window(&w,owner(),1).unwrap();
  assert_eq!(q.pending_owner(owner()),1);
  let dummy=Damage{owner:peer,window:0,rect:Rect{x:0,y:0,w:1,h:1},serial:0};
  let mut empty:[Damage;0]=[];
  assert_eq!(q.drain_owner(owner(),&mut empty),Err(Error::Full));
  let mut got=[dummy;1];
  assert_eq!(q.drain_owner(owner(),&mut got),Ok(1));
  assert_eq!(got[0].rect.w,100);
  assert_eq!(q.pending_owner(peer),1);
 }
 #[test]fn closing_window_reaps_only_matching_damage(){
  let w=windows();let mut q=Surfaces::<4>::new();
  let r=Rect{x:0,y:0,w:3,h:3};
  q.submit_batch(&w,owner(),&[(1,r),(2,r)]).unwrap();
  assert_eq!(q.cancel_window(owner(),1),1);
  assert_eq!(q.pending_owner(owner()),1);
  assert_eq!(q.next().unwrap().window,2);
 }
}
