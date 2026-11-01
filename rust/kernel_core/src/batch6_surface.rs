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


// R084 / compositor workstream: render-queue observability and selective
// atomic draining. Only trusted kernel service code should call these APIs.
impl<const N:usize> Surfaces<N>{
    /// Pending dirty rectangles across all authenticated process owners.
    pub fn pending_total(&self)->usize{self.count}
    pub fn pending_window(&self,owner:Identity,window:u32)->usize{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|m|m.owner==owner&&m.window==window).count()
    }
    pub fn first_pending_serial(&self)->Option<u64>{
        self.messages.first().and_then(|x|*x).map(|x|x.serial)
    }
    pub fn last_pending_serial(&self)->Option<u64>{
        if self.count==0{None}else{
            self.messages[self.count-1].map(|x|x.serial)
        }
    }
    /// Compositor may inspect the next queue head without removing it.
    pub fn peek_oldest(&self)->Option<Damage>{
        self.messages.first().and_then(|x|*x)
    }
    /// Owner-bound peek never discloses a different process's window changes.
    pub fn peek_owner(&self,owner:Identity)->Option<Damage>{
        if owner.pid==0||owner.generation==0{return None;}
        self.messages[..self.count].iter().flatten()
            .find(|m|m.owner==owner).copied()
    }
    /// Drain only a matching owner/window as a transaction; insufficient
    /// output buffers never remove any queue entry.
    pub fn drain_window(&mut self,owner:Identity,window:u32,out:&mut[Damage])
        ->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let wanted=self.pending_window(owner,window);
        if out.len()<wanted{return Err(Error::Full);}
        let mut i=0;let mut used=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&m.window==window){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// Expire obsolete serials for just one process incarnation, leaving
    /// all other owners and newer frame requests untouched.
    pub fn cancel_owner_before(&mut self,owner:Identity,serial:u64)->usize{
        if owner.pid==0||owner.generation==0||serial==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&m.serial<serial){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// Drop stale requests before any renderer can use invalid recycled
    /// window IDs or dirty rectangles outside the CURRENT window geometry.
    pub fn reap_invalid<const W:usize>(&mut self,windows:&Windows<W>)->usize{
        let mut removed=0;let mut i=0;
        while i<self.count{
            let invalid=match self.messages[i]{
                Some(m)=>{
                    if !windows.owns(m.owner.pid,m.owner.generation,m.window){true}
                    else if let Some(current)=windows.get(m.window){
                        let r=m.rect;let bounds=current.rect;
                        r.w==0||r.h==0||r.x.checked_add(r.w).is_none_or(|v|v>bounds.w)||
                        r.y.checked_add(r.h).is_none_or(|v|v>bounds.h)
                    }else{true}
                },
                None=>true
            };
            if invalid{
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// Coalesce a list of updates against the caller's own live windows.
    /// Duplicate windows consolidate without consuming queue slots.
    /// Any invalid, foreign or overflowed request rolls back the whole batch.
    pub fn coalesce_batch<const W:usize>(&mut self,windows:&Windows<W>,
        owner:Identity,updates:&[(u32,Rect)])->Result<usize,Error>{
        if updates.is_empty()||updates.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        for &(window,rect) in updates{
            staged.submit_coalesced(windows,owner,window,rect)?;
        }
        self.messages=staged.messages;self.count=staged.count;self.next=staged.next;
        Ok(updates.len())
    }
}
#[cfg(test)]
mod r084_compositor_tests{
    use super::*;
    use crate::batch2_windows::Window;
    fn owner()->Identity{Identity{pid:90,generation:4}}
    fn peer()->Identity{Identity{pid:91,generation:4}}
    fn windows()->Windows<3>{
        let mut x=Windows::new();
        x.create(Window{pid:90,generation:4,id:7,rect:Rect{x:0,y:0,w:40,h:30}}).unwrap();
        x.create(Window{pid:90,generation:4,id:8,rect:Rect{x:50,y:20,w:30,h:30}}).unwrap();
        x.create(Window{pid:91,generation:4,id:9,rect:Rect{x:100,y:20,w:30,h:30}}).unwrap();
        x
    }
    fn rect()->Rect{Rect{x:2,y:3,w:4,h:5}}
    #[test]
    fn coalescing_is_transactional_and_preflighted_window_drain_is_scoped(){
        let w=windows();let mut q=Surfaces::<4>::new();
        assert_eq!(q.coalesce_batch(&w,owner(),&[(7,rect()),(7,Rect{x:4,..rect()}),(8,rect())]),Ok(3));
        assert_eq!(q.pending_total(),2);
        assert_eq!(q.pending_window(owner(),7),1);
        assert_eq!(q.first_pending_serial(),Some(1));
        assert_eq!(q.last_pending_serial(),Some(2));
        assert_eq!(q.peek_oldest().unwrap().rect.w,6);
        assert_eq!(q.peek_owner(owner()).unwrap().window,7);
        let before=q.peek_oldest();
        assert_eq!(q.coalesce_batch(&w,owner(),&[(7,rect()),(9,rect())]),Err(Error::NotOwner));
        assert_eq!(q.peek_oldest(),before);
        let dummy=q.peek_oldest().unwrap();let mut short:[Damage;0]=[];
        assert_eq!(q.drain_window(owner(),7,&mut short),Err(Error::Full));
        let mut out=[dummy;2];
        assert_eq!(q.drain_window(owner(),7,&mut out),Ok(1));
        assert_eq!(out[0].rect.w,6);
        assert_eq!(q.pending_window(owner(),8),1);
        assert_eq!(q.pending_total(),1);
        assert_eq!(q.cancel_owner_before(owner(),3),1);
        assert_eq!(q.pending_total(),0);
    }
    #[test]
    fn retired_window_or_generation_reaps_without_discarding_peers(){
        let mut w=windows();let mut q=Surfaces::<4>::new();
        q.submit(&w,owner(),7,rect()).unwrap();
        q.submit(&w,peer(),9,rect()).unwrap();
        assert_eq!(q.reap_invalid(&w),0);
        w.retire(owner().pid,owner().generation);
        assert_eq!(q.reap_invalid(&w),1);
        assert_eq!(q.pending_owner(peer()),1);
        assert_eq!(q.peek_owner(owner()),None);
        assert_eq!(q.pending_total(),1);
    }
}


// R084 extended compositor queue services. Changes are owner/epoch-bound,
// transactional and limited to existing live-window damage messages.
impl<const N:usize> Surfaces<N>{
    /// 1. A serial is visible only to the exact generation that submitted it.
    pub fn owner_has_serial(&self,owner:Identity,serial:u64)->bool{
        owner.pid!=0&&owner.generation!=0&&serial!=0&&
        self.messages[..self.count].iter().flatten().any(|d|
            d.owner==owner&&d.serial==serial)
    }
    /// 2. Oldest queued window ID for this exact application incarnation.
    pub fn owner_first_window(&self,owner:Identity)->Option<u32>{
        self.peek_owner(owner).map(|d|d.window)
    }
    /// 3. Most recent queued window ID without leaking other owners.
    pub fn owner_last_window(&self,owner:Identity)->Option<u32>{
        if owner.pid==0||owner.generation==0{return None;}
        self.messages[..self.count].iter().rev().flatten()
            .find(|d|d.owner==owner).map(|d|d.window)
    }
    /// 4. Oldest serial of an exact owner's window.
    pub fn window_first_serial(&self,owner:Identity,window:u32)->Option<u64>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().flatten()
            .find(|d|d.owner==owner&&d.window==window).map(|d|d.serial)
    }
    /// 5. Newest serial of an exact owner's window.
    pub fn window_last_serial(&self,owner:Identity,window:u32)->Option<u64>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().rev().flatten()
            .find(|d|d.owner==owner&&d.window==window).map(|d|d.serial)
    }
    /// 6. Count queued updates not older than an observed serial.
    pub fn owner_pending_since(&self,owner:Identity,serial:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|d|d.owner==owner&&d.serial>=serial).count()
    }
    /// 7. Count old updates which can safely expire for one process.
    pub fn owner_pending_before(&self,owner:Identity,serial:u64)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|d|d.owner==owner&&d.serial<serial).count()
    }
    /// 8. Surface damage area is saturating: cannot wrap in diagnostics.
    pub fn window_pending_area(&self,owner:Identity,window:u32)->u64{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|d|d.owner==owner&&d.window==window)
            .fold(0u64,|n,d|n.saturating_add(d.rect.w as u64*d.rect.h as u64))
    }
    /// 9. Owner total damage area without using a raw framebuffer pointer.
    pub fn owner_pending_area(&self,owner:Identity)->u64{
        if owner.pid==0||owner.generation==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|d|d.owner==owner)
            .fold(0u64,|n,d|n.saturating_add(d.rect.w as u64*d.rect.h as u64))
    }
    /// 10. Atomic owner/window inventory in render submission order.
    pub fn snapshot_window(&self,owner:Identity,window:u32,out:&mut[Damage])
        ->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let need=self.pending_window(owner,window);
        if out.len()<need{return Err(Error::Full);}
        let mut n=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.window==window{out[n]=*m;n+=1;}
        }
        Ok(n)
    }
    /// 11. Withdraw just one owner-authenticated damage serial.
    pub fn cancel_exact_serial(&mut self,owner:Identity,serial:u64)->bool{
        if !self.owner_has_serial(owner,serial){return false;}
        let Some(index)=self.messages[..self.count].iter().position(|d|
            d.is_some_and(|m|m.owner==owner&&m.serial==serial))else{return false;};
        for j in index+1..self.count{self.messages[j-1]=self.messages[j];}
        self.count-=1;self.messages[self.count]=None;true
    }
    /// 12. Cancel this owner's serials from a newer epoch onward.
    pub fn cancel_owner_from(&mut self,owner:Identity,from:u64)->usize{
        if owner.pid==0||owner.generation==0||from==0{return 0;}
        let mut total=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&m.serial>=from){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;total+=1;
            }else{i+=1;}
        }
        total
    }
    /// 13. Cancel only an inclusive serial range for this owner.
    pub fn cancel_owner_range(&mut self,owner:Identity,from:u64,to:u64)->usize{
        if owner.pid==0||owner.generation==0||from==0||to<from{return 0;}
        let mut total=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|
                m.owner==owner&&m.serial>=from&&m.serial<=to){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;total+=1;
            }else{i+=1;}
        }
        total
    }
    /// 14. Reap only later updates to an exact window.
    pub fn cancel_window_from(&mut self,owner:Identity,window:u32,from:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0||from==0{return 0;}
        let mut total=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|
                m.owner==owner&&m.window==window&&m.serial>=from){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;total+=1;
            }else{i+=1;}
        }
        total
    }
    /// 15. Batch retirement validates distinct window IDs before mutation.
    pub fn cancel_windows_batch(&mut self,owner:Identity,windows:&[u32])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||windows.is_empty()||
            windows.len()>N{return Err(Error::Invalid);}
        for (i,&w) in windows.iter().enumerate(){
            if w==0||windows[..i].contains(&w){return Err(Error::Invalid);}
        }
        let mut total=0;
        for &w in windows{total+=self.cancel_window(owner,w);}
        Ok(total)
    }
    /// 16. Publish and drain all this owner's earlier damage in one step.
    pub fn drain_owner_before(&mut self,owner:Identity,before:u64,out:&mut[Damage])
        ->Result<usize,Error>{
        if owner.pid==0||owner.generation==0{return Err(Error::NotOwner);}
        let required=self.owner_pending_before(owner,before);
        if out.len()<required{return Err(Error::Full);}
        let mut used=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&m.serial<before){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// 17. Drain earlier updates from one window, preserving owner peers.
    pub fn drain_window_before(&mut self,owner:Identity,window:u32,before:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let required=self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial<before).count();
        if out.len()<required{return Err(Error::Full);}
        let mut used=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|
                m.owner==owner&&m.window==window&&m.serial<before){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// 18. Owner-valid clipped dirty rectangles are submitted atomically.
    pub fn submit_clipped_batch<const W:usize>(&mut self,w:&Windows<W>,owner:Identity,
        changes:&[(u32,Rect)])->Result<usize,Error>{
        if changes.is_empty()||changes.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        for &(window,rect) in changes{staged.submit_clipped(w,owner,window,rect)?;}
        *self=staged;Ok(changes.len())
    }
    /// 19. Request full-window redraw of a set of live owner windows.
    pub fn redraw_batch<const W:usize>(&mut self,w:&Windows<W>,owner:Identity,
        window_ids:&[u32])->Result<usize,Error>{
        if window_ids.is_empty()||window_ids.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        for (i,&window) in window_ids.iter().enumerate(){
            if window==0||window_ids[..i].contains(&window){return Err(Error::Invalid);}
            staged.redraw_window(w,owner,window)?;
        }
        *self=staged;Ok(window_ids.len())
    }
    /// 20. Modify a pending message only on exact owner/serial/window and
    /// current geometry; an invalid rect never replaces a valid queued rect.
    pub fn replace_pending_rect<const W:usize>(&mut self,w:&Windows<W>,
        owner:Identity,window:u32,serial:u64,rect:Rect)->Result<(),Error>{
        if owner.pid==0||owner.generation==0||!w.owns(owner.pid,owner.generation,window){
            return Err(Error::NotOwner);
        }
        let geometry=w.get(window).ok_or(Error::NotOwner)?.rect;
        if rect.w==0||rect.h==0||
            rect.x.checked_add(rect.w).is_none_or(|v|v>geometry.w)||
            rect.y.checked_add(rect.h).is_none_or(|v|v>geometry.h){return Err(Error::Invalid);}
        let existing=self.messages[..self.count].iter_mut().flatten().find(|d|
            d.owner==owner&&d.window==window&&d.serial==serial)
            .ok_or(Error::NotOwner)?;
        existing.rect=rect;Ok(())
    }
}
#[cfg(test)]
mod r084_extended_surface_tests{
    use super::*;use crate::batch2_windows::Window;
    fn owner()->Identity{Identity{pid:101,generation:7}}
    fn peer()->Identity{Identity{pid:102,generation:7}}
    fn windows()->Windows<3>{
        let mut w=Windows::new();
        w.create(Window{pid:101,generation:7,id:1,rect:Rect{x:0,y:0,w:40,h:40}}).unwrap();
        w.create(Window{pid:101,generation:7,id:2,rect:Rect{x:50,y:0,w:40,h:40}}).unwrap();
        w.create(Window{pid:102,generation:7,id:3,rect:Rect{x:100,y:0,w:40,h:40}}).unwrap();w
    }
    fn rect()->Rect{Rect{x:0,y:0,w:4,h:5}}
    #[test]
    fn twenty_owner_scoped_damage_services_enforce_atomicity_and_window_epochs(){
        let w=windows();let mut q=Surfaces::<8>::new();
        assert_eq!(q.submit_clipped_batch(&w,owner(),&[(1,rect()),(2,rect())]),Ok(2));
        let serial3=q.submit(&w,peer(),3,rect()).unwrap();
        assert!(q.owner_has_serial(owner(),1));
        assert!(!q.owner_has_serial(peer(),1));
        assert_eq!(q.owner_first_window(owner()),Some(1));
        assert_eq!(q.owner_last_window(owner()),Some(2));
        assert_eq!(q.window_first_serial(owner(),1),Some(1));
        assert_eq!(q.window_last_serial(owner(),2),Some(2));
        assert_eq!(q.owner_pending_since(owner(),2),1);
        assert_eq!(q.owner_pending_before(owner(),2),1);
        assert_eq!(q.window_pending_area(owner(),1),20);
        assert_eq!(q.owner_pending_area(owner()),40);
        let filler=q.peek_oldest().unwrap();let mut tiny=[filler;0];
        assert_eq!(q.snapshot_window(owner(),1,&mut tiny),Err(Error::Full));
        let mut out=[filler;8];assert_eq!(q.snapshot_window(owner(),1,&mut out),Ok(1));
        assert_eq!(q.redraw_batch(&w,owner(),&[1,2]),Ok(2));
        assert_eq!(q.pending_owner(owner()),2);
        assert_eq!(q.submit_clipped_batch(&w,owner(),
            &[(1,rect()),(3,rect())]),Err(Error::NotOwner));
        assert_eq!(q.pending_owner(owner()),2);
        assert!(q.replace_pending_rect(&w,owner(),1,1,Rect{x:1,y:1,w:5,h:5}).is_ok());
        assert!(q.cancel_exact_serial(owner(),1));
        assert!(!q.cancel_exact_serial(peer(),2));
        assert_eq!(q.drain_owner_before(owner(),serial3,&mut out),Ok(1));
        assert_eq!(q.pending_owner(owner()),0);
        assert_eq!(q.pending_owner(peer()),1);
        assert_eq!(q.cancel_owner_range(peer(),serial3,serial3),1);
        assert_eq!(q.pending_total(),0);
    }
    #[test]
    fn range_filters_and_window_batches_never_drop_foreign_updates(){
        let w=windows();let mut q=Surfaces::<8>::new();
        q.submit(&w,owner(),1,rect()).unwrap();q.submit(&w,owner(),2,rect()).unwrap();
        q.submit(&w,peer(),3,rect()).unwrap();
        assert_eq!(q.cancel_windows_batch(owner(),&[1,1]),Err(Error::Invalid));
        assert_eq!(q.pending_owner(owner()),2);
        assert_eq!(q.cancel_window_from(owner(),1,1),1);
        assert_eq!(q.cancel_owner_from(owner(),2),1);
        assert_eq!(q.pending_owner(peer()),1);
        let filler=q.peek_oldest().unwrap();let mut out=[filler;2];
        assert_eq!(q.drain_window_before(peer(),3,4,&mut out),Ok(1));
        assert_eq!(q.pending_total(),0);
    }
}


// R084 extension III: compositor visibility, geometry preflight, and
// owner-generation-scoped damage edits. No raw pixels exposed.
impl<const N:usize> Surfaces<N>{
    /// 1. Number of distinct live owner epochs with queued frame requests.
    pub fn queued_owner_count(&self)->usize{
        let mut count=0;
        for i in 0..self.count{
            let Some(d)=self.messages[i]else{continue;};
            if !self.messages[..i].iter().flatten().any(|x|x.owner==d.owner){
                count+=1;
            }
        }
        count
    }
    /// 2. Count distinct windows with pending update requests.
    pub fn queued_window_count(&self)->usize{
        let mut count=0;
        for i in 0..self.count{
            let Some(d)=self.messages[i]else{continue;};
            if !self.messages[..i].iter().flatten().any(|x|
                x.owner==d.owner&&x.window==d.window){count+=1;}
        }
        count
    }
    /// 3. Count this process's distinct pending windows, not rectangles.
    pub fn owner_window_count(&self,owner:Identity)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        let mut count=0;
        for i in 0..self.count{
            let Some(d)=self.messages[i]else{continue;};
            if d.owner!=owner{continue;}
            if !self.messages[..i].iter().flatten().any(|x|
                x.owner==owner&&x.window==d.window){count+=1;}
        }
        count
    }
    /// 4. Count pending rectangles which overlap an owner-visible region.
    pub fn overlapping_window_count(&self,owner:Identity,window:u32,query:Rect)->usize{
        if owner.pid==0||owner.generation==0||window==0||query.w==0||query.h==0{
            return 0;
        }
        let Some(qx)=query.x.checked_add(query.w)else{return 0;};
        let Some(qy)=query.y.checked_add(query.h)else{return 0;};
        self.messages[..self.count].iter().flatten().filter(|m|{
            let r=m.rect;
            m.owner==owner&&m.window==window&&
            r.x<qx&&query.x<r.x.saturating_add(r.w)&&
            r.y<qy&&query.y<r.y.saturating_add(r.h)
        }).count()
    }
    /// 5. Count damage rectangles containing one local pixel coordinate.
    pub fn window_damage_at(&self,owner:Identity,window:u32,x:u32,y:u32)->usize{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|{
            let r=m.rect;
            m.owner==owner&&m.window==window&&x>=r.x&&
            x<r.x.saturating_add(r.w)&&y>=r.y&&y<r.y.saturating_add(r.h)
        }).count()
    }
    /// 6. Bounded union of all rectangles for one owner/window.
    pub fn window_damage_bounds(&self,owner:Identity,window:u32)->Option<Rect>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        let mut union:Option<Rect>=None;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner!=owner||m.window!=window{continue;}
            let r=m.rect;
            union=Some(match union{
                None=>r,
                Some(old)=>{
                    let x=old.x.min(r.x);let y=old.y.min(r.y);
                    let right=old.x.saturating_add(old.w).max(r.x.saturating_add(r.w));
                    let bottom=old.y.saturating_add(old.h).max(r.y.saturating_add(r.h));
                    Rect{x,y,w:right-x,h:bottom-y}
                }
            });
        }
        union
    }
    /// 7. Oldest owner-visible pending damage without removing it.
    pub fn oldest_owner_damage(&self,owner:Identity)->Option<Damage>{
        self.peek_owner(owner)
    }
    /// 8. Newest owner-visible damage without disclosing peer information.
    pub fn newest_owner_damage(&self,owner:Identity)->Option<Damage>{
        if owner.pid==0||owner.generation==0{return None;}
        self.messages[..self.count].iter().rev().flatten()
            .find(|m|m.owner==owner).copied()
    }
    /// 9. Non-consuming lookup by exact owner/serial generation.
    pub fn damage_at_serial(&self,owner:Identity,serial:u64)->Option<Damage>{
        if owner.pid==0||owner.generation==0||serial==0{return None;}
        self.messages[..self.count].iter().flatten()
            .find(|m|m.owner==owner&&m.serial==serial).copied()
    }
    /// 10. Count pending serials in one inclusive range for a specific owner.
    pub fn owner_range_count(&self,owner:Identity,first:u64,last:u64)->usize{
        if owner.pid==0||owner.generation==0||first>last{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.serial>=first&&m.serial<=last).count()
    }
    /// 11. Read current queue capacity without exposing other messages.
    pub fn free_message_slots(&self)->usize{N.saturating_sub(self.count)}
    /// 12. Inspect whether any work exists for the exact process epoch.
    pub fn owner_has_damage(&self,owner:Identity)->bool{
        self.pending_owner(owner)>0
    }
    /// 13. Remove one serial and write its original message, fail on short
    /// output without destroying the queued message.
    pub fn drain_serial(&mut self,owner:Identity,serial:u64,out:&mut[Damage])
        ->Result<usize,Error>{
        let msg=self.damage_at_serial(owner,serial).ok_or(Error::NotOwner)?;
        if out.is_empty(){return Err(Error::Full);}
        if !self.cancel_exact_serial(owner,serial){return Err(Error::NotOwner);}
        out[0]=msg;Ok(1)
    }
    /// 14. Atomic owner serial-range extraction with complete output preflight.
    pub fn drain_serial_range(&mut self,owner:Identity,first:u64,last:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||first==0||last<first{
            return Err(Error::Invalid);
        }
        let needed=self.owner_range_count(owner,first,last);
        if out.len()<needed{return Err(Error::Full);}
        let mut at=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&
                m.serial>=first&&m.serial<=last){
                out[at]=self.messages[i].ok_or(Error::Empty)?;at+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(at)
    }
    /// 15. Remove only a selected owner's window messages older than serial.
    pub fn cancel_window_before(&mut self,owner:Identity,window:u32,before:u64)
        ->usize{
        if owner.pid==0||owner.generation==0||window==0||before==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&
                m.window==window&&m.serial<before){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// 16. Exact stale expected rectangle prevents wrong-window damage edits.
    pub fn replace_rect_if_equal<const W:usize>(&mut self,w:&Windows<W>,
        owner:Identity,window:u32,serial:u64,expected:Rect,next:Rect)
        ->Result<(),Error>{
        let existing=self.damage_at_serial(owner,serial).ok_or(Error::NotOwner)?;
        if existing.window!=window||existing.rect!=expected{return Err(Error::Invalid);}
        self.replace_pending_rect(w,owner,window,serial,next)
    }
    /// 17. Union a newer dirty rectangle into one exact existing serial.
    pub fn extend_damage<const W:usize>(&mut self,w:&Windows<W>,owner:Identity,
        window:u32,serial:u64,extra:Rect)->Result<(),Error>{
        let old=self.damage_at_serial(owner,serial).ok_or(Error::NotOwner)?;
        if old.window!=window||extra.w==0||extra.h==0{return Err(Error::Invalid);}
        let right=old.rect.x.checked_add(old.rect.w).ok_or(Error::Invalid)?
            .max(extra.x.checked_add(extra.w).ok_or(Error::Invalid)?);
        let bottom=old.rect.y.checked_add(old.rect.h).ok_or(Error::Invalid)?
            .max(extra.y.checked_add(extra.h).ok_or(Error::Invalid)?);
        let x=old.rect.x.min(extra.x);let y=old.rect.y.min(extra.y);
        self.replace_pending_rect(w,owner,window,serial,
            Rect{x,y,w:right-x,h:bottom-y})
    }
    /// 18. Intersect one already queued damage rect with a visible selection.
    pub fn intersect_damage<const W:usize>(&mut self,w:&Windows<W>,owner:Identity,
        window:u32,serial:u64,clip:Rect)->Result<(),Error>{
        let old=self.damage_at_serial(owner,serial).ok_or(Error::NotOwner)?;
        if old.window!=window||clip.w==0||clip.h==0{return Err(Error::Invalid);}
        let x=old.rect.x.max(clip.x);let y=old.rect.y.max(clip.y);
        let right=old.rect.x.checked_add(old.rect.w).ok_or(Error::Invalid)?
            .min(clip.x.checked_add(clip.w).ok_or(Error::Invalid)?);
        let bottom=old.rect.y.checked_add(old.rect.h).ok_or(Error::Invalid)?
            .min(clip.y.checked_add(clip.h).ok_or(Error::Invalid)?);
        if right<=x||bottom<=y{return Err(Error::Invalid);}
        self.replace_pending_rect(w,owner,window,serial,
            Rect{x,y,w:right-x,h:bottom-y})
    }
    /// 19. Move an existing dirty rect by a signed local delta with checked
    /// bounds (never moves the corresponding window or leaks screen pixels).
    pub fn translate_damage<const W:usize>(&mut self,w:&Windows<W>,owner:Identity,
        window:u32,serial:u64,dx:i32,dy:i32)->Result<(),Error>{
        let old=self.damage_at_serial(owner,serial).ok_or(Error::NotOwner)?;
        if old.window!=window{return Err(Error::Invalid);}
        let x=(old.rect.x as i64).checked_add(dx as i64)
            .filter(|v|*v>=0&&*v<=u32::MAX as i64).ok_or(Error::Invalid)? as u32;
        let y=(old.rect.y as i64).checked_add(dy as i64)
            .filter(|v|*v>=0&&*v<=u32::MAX as i64).ok_or(Error::Invalid)? as u32;
        self.replace_pending_rect(w,owner,window,serial,
            Rect{x,y,w:old.rect.w,h:old.rect.h})
    }
    /// 20. Submit multiple clipped/coalesced changes as one transaction.
    pub fn coalesce_clipped_batch<const W:usize>(&mut self,w:&Windows<W>,
        owner:Identity,changes:&[(u32,Rect)])->Result<usize,Error>{
        if changes.is_empty()||changes.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        for &(window,r) in changes{
            if !w.owns(owner.pid,owner.generation,window){return Err(Error::NotOwner);}
            let size=w.get(window).ok_or(Error::NotOwner)?.rect;
            if r.w==0||r.h==0||r.x>=size.w||r.y>=size.h{return Err(Error::Invalid);}
            let right=r.x.saturating_add(r.w).min(size.w);
            let bottom=r.y.saturating_add(r.h).min(size.h);
            staged.submit_coalesced(w,owner,window,
                Rect{x:r.x,y:r.y,w:right-r.x,h:bottom-r.y})?;
        }
        *self=staged;Ok(changes.len())
    }
}
#[cfg(test)]
mod r084_damage_geometry_batch_tests{
    use super::*;
    use crate::batch2_windows::Window;
    fn owner()->Identity{Identity{pid:120,generation:3}}
    fn peer()->Identity{Identity{pid:121,generation:3}}
    fn windows()->Windows<3>{
        let mut w=Windows::new();
        w.create(Window{pid:120,generation:3,id:1,
            rect:Rect{x:0,y:0,w:50,h:50}}).unwrap();
        w.create(Window{pid:120,generation:3,id:2,
            rect:Rect{x:70,y:0,w:50,h:50}}).unwrap();
        w.create(Window{pid:121,generation:3,id:3,
            rect:Rect{x:150,y:0,w:50,h:50}}).unwrap();w
    }
    fn rect()->Rect{Rect{x:2,y:2,w:5,h:5}}
    #[test]
    fn queue_ownership_geometry_and_pinned_damage_transitions(){
        let w=windows();let mut q=Surfaces::<8>::new();
        q.submit(&w,owner(),1,rect()).unwrap();
        q.submit(&w,owner(),2,rect()).unwrap();
        q.submit(&w,peer(),3,rect()).unwrap();
        assert_eq!(q.queued_owner_count(),2);
        assert_eq!(q.queued_window_count(),3);
        assert_eq!(q.owner_window_count(owner()),2);
        assert_eq!(q.overlapping_window_count(owner(),1,rect()),1);
        assert_eq!(q.window_damage_at(owner(),1,3,3),1);
        assert_eq!(q.window_damage_bounds(owner(),1),Some(rect()));
        assert_eq!(q.oldest_owner_damage(owner()).unwrap().serial,1);
        assert_eq!(q.newest_owner_damage(owner()).unwrap().serial,2);
        assert_eq!(q.damage_at_serial(owner(),1).unwrap().window,1);
        assert_eq!(q.owner_range_count(owner(),1,2),2);
        assert_eq!(q.free_message_slots(),5);
        assert!(q.owner_has_damage(owner()));
        q.extend_damage(&w,owner(),1,1,Rect{x:6,y:6,w:4,h:4}).unwrap();
        assert_eq!(q.window_damage_bounds(owner(),1),Some(Rect{x:2,y:2,w:8,h:8}));
        q.intersect_damage(&w,owner(),1,1,Rect{x:4,y:4,w:4,h:4}).unwrap();
        q.translate_damage(&w,owner(),1,1,1,1).unwrap();
        let current=q.damage_at_serial(owner(),1).unwrap().rect;
        assert_eq!(current,Rect{x:5,y:5,w:4,h:4});
        assert_eq!(q.replace_rect_if_equal(&w,owner(),1,1,rect(),rect()),Err(Error::Invalid));
        assert_eq!(q.coalesce_clipped_batch(&w,owner(),
            &[(1,Rect{x:48,y:48,w:50,h:50})]),Ok(1));
        assert_eq!(q.queued_window_count(),3);
        let filler=q.peek_oldest().unwrap();
        let mut short:[Damage;0]=[];
        assert_eq!(q.drain_serial(owner(),1,&mut short),Err(Error::Full));
        let mut out=[filler;8];
        assert_eq!(q.drain_serial(owner(),1,&mut out),Ok(1));
        assert_eq!(q.cancel_window_before(owner(),2,3),1);
        assert_eq!(q.drain_serial_range(peer(),3,3,&mut out),Ok(1));
        assert_eq!(q.pending_total(),0);
    }
}


// M1/R084 extension IV — 20 serial-ordered compositor mailbox operations.
// Caller identity must be from trusted syscall ingress; no raw pixel access.
impl<const N:usize> Surfaces<N>{
    /// 1. Saturated global queued damage area (trusted compositor metric).
    pub fn queued_area_total(&self)->u64{
        self.messages[..self.count].iter().flatten().fold(0u64,|n,m|
            n.saturating_add(m.rect.w as u64*m.rect.h as u64))
    }
    /// 2. Count events for one process strictly after a known serial.
    pub fn owner_count_after(&self,who:Identity,serial:u64)->usize{
        if who.pid==0||who.generation==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|m|m.owner==who&&m.serial>serial).count()
    }
    /// 3. Count owner events up to and including observed serial.
    pub fn owner_count_through(&self,who:Identity,serial:u64)->usize{
        if who.pid==0||who.generation==0{return 0;}
        self.messages[..self.count].iter().flatten()
            .filter(|m|m.owner==who&&m.serial<=serial).count()
    }
    /// 4. Verify serial uniqueness in the global damage queue.
    pub fn serial_exists(&self,serial:u64)->bool{
        serial!=0&&self.messages[..self.count].iter().flatten()
            .any(|m|m.serial==serial)
    }
    /// 5. Verify exact process incarnation, window and serial.
    pub fn serial_is_window(&self,who:Identity,window:u32,serial:u64)->bool{
        if window==0{return false;}
        self.damage_at_serial(who,serial).is_some_and(|m|m.window==window)
    }
    /// 6. Oldest existing damage on a given authorized window.
    pub fn oldest_window_damage(&self,who:Identity,window:u32)->Option<Damage>{
        if who.pid==0||who.generation==0||window==0{return None;}
        self.messages[..self.count].iter().flatten()
            .find(|m|m.owner==who&&m.window==window).copied()
    }
    /// 7. Newest authorized window damage, without draining it.
    pub fn newest_window_damage(&self,who:Identity,window:u32)->Option<Damage>{
        if who.pid==0||who.generation==0||window==0{return None;}
        self.messages[..self.count].iter().rev().flatten()
            .find(|m|m.owner==who&&m.window==window).copied()
    }
    /// 8. Atomic snapshot of a process's events after an observed serial.
    pub fn snapshot_owner_after(&self,who:Identity,serial:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if who.pid==0||who.generation==0{return Err(Error::NotOwner);}
        let need=self.owner_count_after(who,serial);
        if out.len()<need{return Err(Error::Full);}
        let mut at=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==who&&m.serial>serial{out[at]=*m;at+=1;}
        }
        Ok(at)
    }
    /// 9. Trusted compositor global snapshot, short destination publishes none.
    pub fn snapshot_global(&self,out:&mut[Damage])->Result<usize,Error>{
        if out.len()<self.count{return Err(Error::Full);}
        for (i,m) in self.messages[..self.count].iter().enumerate(){
            out[i]=m.ok_or(Error::Empty)?;
        }
        Ok(self.count)
    }
    /// 10. Verify all populated slots are dense with unique ascending serials.
    pub fn verify_damage_queue(&self)->bool{
        if self.count>N{return false;}
        let mut last=0;
        for m in &self.messages[..self.count]{
            let Some(d)=m else{return false;};
            if d.owner.pid==0||d.owner.generation==0||d.window==0||
               d.serial==0||d.serial<=last{return false;}
            last=d.serial;
        }
        self.messages[self.count..].iter().all(Option::is_none)
    }
    /// 11. Cancel exactly one serial only on the matching owned window.
    pub fn cancel_window_serial(&mut self,who:Identity,window:u32,serial:u64)->bool{
        if !self.serial_is_window(who,window,serial){return false;}
        self.cancel_exact_serial(who,serial)
    }
    /// 12. Drop only a serial interval from one owner-visible window.
    pub fn cancel_window_range(&mut self,who:Identity,window:u32,
        first:u64,last:u64)->usize{
        if who.pid==0||who.generation==0||window==0||first==0||last<first{
            return 0;
        }
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==who&&m.window==window&&
                m.serial>=first&&m.serial<=last){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// 13. Drain events after a serial, preserving every peer process.
    pub fn drain_owner_after(&mut self,who:Identity,serial:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if who.pid==0||who.generation==0{return Err(Error::NotOwner);}
        let need=self.owner_count_after(who,serial);
        if out.len()<need{return Err(Error::Full);}
        let mut used=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==who&&m.serial>serial){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// 14. Drain one window's newer work, not a peer window's notifications.
    pub fn drain_window_after(&mut self,who:Identity,window:u32,serial:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if who.pid==0||who.generation==0||window==0{return Err(Error::NotOwner);}
        let need=self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==who&&m.window==window&&m.serial>serial).count();
        if out.len()<need{return Err(Error::Full);}
        let mut used=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==who&&m.window==window&&
                m.serial>serial){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// 15. Kernel compositor drops only globally obsolete serials.
    pub fn discard_before(&mut self,serial:u64)->usize{
        if serial==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.serial<serial){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// 16. Drop completed <= serial for trusted compositor acknowledgement.
    pub fn discard_through(&mut self,serial:u64)->usize{
        if serial==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.serial<=serial){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// 17. Remove owner-completed work without touching peer generations.
    pub fn acknowledge_owner_through(&mut self,who:Identity,serial:u64)->usize{
        if serial==0{return 0;}
        self.cancel_owner_range(who,1,serial)
    }
    /// 18. Atomic ordered publication of multiple new damage serials.
    pub fn submit_batch_with_serials<const W:usize>(&mut self,w:&Windows<W>,
        who:Identity,req:&[(u32,Rect)],out:&mut[u64])->Result<usize,Error>{
        if req.is_empty()||req.len()>N{return Err(Error::Invalid);}
        if out.len()<req.len(){return Err(Error::Full);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        let mut serials=[0u64;N];
        for (i,&(window,rect)) in req.iter().enumerate(){
            serials[i]=staged.submit(w,who,window,rect)?;
        }
        *self=staged;out[..req.len()].copy_from_slice(&serials[..req.len()]);
        Ok(req.len())
    }
    /// 19. Transactional clipped publication, preserving serial ordering.
    pub fn submit_clipped_with_serials<const W:usize>(&mut self,w:&Windows<W>,
        who:Identity,req:&[(u32,Rect)],out:&mut[u64])->Result<usize,Error>{
        if req.is_empty()||req.len()>N{return Err(Error::Invalid);}
        if out.len()<req.len(){return Err(Error::Full);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        let mut serials=[0u64;N];
        for (i,&(window,rect)) in req.iter().enumerate(){
            serials[i]=staged.submit_clipped(w,who,window,rect)?;
        }
        *self=staged;out[..req.len()].copy_from_slice(&serials[..req.len()]);
        Ok(req.len())
    }
    /// 20. Full-window invalidation only when no work for that window exists.
    pub fn redraw_window_if_idle<const W:usize>(&mut self,w:&Windows<W>,
        who:Identity,window:u32)->Result<Option<u64>,Error>{
        if who.pid==0||who.generation==0||window==0||
            !w.owns(who.pid,who.generation,window){return Err(Error::NotOwner);}
        if self.pending_window(who,window)!=0{return Ok(None);}
        let id=self.redraw_window(w,who,window)?;
        Ok(Some(id))
    }
}
#[cfg(test)]
mod r084_damage_publication_batch_tests{
    use super::*;use crate::batch2_windows::Window;
    fn id()->Identity{Identity{pid:201,generation:2}}
    fn peer()->Identity{Identity{pid:202,generation:2}}
    fn w()->Windows<2>{
        let mut w=Windows::new();
        w.create(Window{pid:201,generation:2,id:1,rect:Rect{x:0,y:0,w:40,h:40}}).unwrap();
        w.create(Window{pid:202,generation:2,id:2,rect:Rect{x:50,y:0,w:40,h:40}}).unwrap();w
    }
    fn rect()->Rect{Rect{x:2,y:3,w:4,h:5}}
    #[test]
    fn damage_publish_snapshot_drain_and_ack_preserve_owner_identity(){
        let windows=w();let mut q=Surfaces::<8>::new();
        let mut ids=[0u64;3];
        assert_eq!(q.submit_batch_with_serials(&windows,id(),&[(1,rect()),(1,rect())],
           &mut ids),Ok(2));
        assert_eq!(&ids[..2],&[1,2]);
        assert_eq!(q.submit_clipped_with_serials(&windows,peer(),
            &[(2,Rect{x:39,y:39,w:8,h:8})],&mut ids),Ok(1));
        assert_eq!(ids[0],3);
        assert_eq!(q.queued_area_total(),41);
        assert_eq!(q.owner_count_after(id(),1),1);
        assert_eq!(q.owner_count_through(id(),1),1);
        assert!(q.serial_exists(3));
        assert!(q.serial_is_window(id(),1,2));
        assert_eq!(q.oldest_window_damage(id(),1).unwrap().serial,1);
        assert_eq!(q.newest_window_damage(id(),1).unwrap().serial,2);
        assert!(q.verify_damage_queue());
        let filler=q.peek_oldest().unwrap();
        let mut short:[Damage;0]=[];
        assert_eq!(q.snapshot_owner_after(id(),0,&mut short),Err(Error::Full));
        let mut items=[filler;8];
        assert_eq!(q.snapshot_global(&mut items),Ok(3));
        assert_eq!(q.drain_window_after(id(),1,1,&mut items),Ok(1));
        assert_eq!(items[0].serial,2);
        assert_eq!(q.acknowledge_owner_through(id(),1),1);
        assert_eq!(q.pending_owner(peer()),1);
        assert_eq!(q.discard_before(3),0);
        assert_eq!(q.discard_through(3),1);
        assert_eq!(q.pending_total(),0);
        assert!(q.redraw_window_if_idle(&windows,id(),1).unwrap().is_some());
    }
    #[test]
    fn invalid_batch_never_consumes_sequence_or_publishes_partial_output(){
        let w=w();let mut q=Surfaces::<3>::new();
        let mut out=[77u64;2];
        assert_eq!(q.submit_batch_with_serials(&w,id(),
          &[(1,rect()),(2,rect())],&mut out),Err(Error::NotOwner));
        assert_eq!(out,[77;2]);
        assert_eq!(q.pending_total(),0);
        assert!(!q.serial_exists(1));
        assert_eq!(q.submit_clipped_with_serials(&w,id(),&[(1,rect())],&mut[]),
            Err(Error::Full));
        assert_eq!(q.drain_owner_after(Identity{generation:0,..id()},0,
            &mut[]),Err(Error::NotOwner));
    }
}


// R084 extension V: 20 identity-scoped frame-damage queue queries and
// immutable-serial lifecycle transitions for the live compositor mailbox.
impl<const N:usize> Surfaces<N>{
    /// Number of queued updates in a single window newer than a serial.
    pub fn window_count_after(&self,owner:Identity,window:u32,serial:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial>serial).count()
    }
    /// Number of queued updates up through a serial in one owner window.
    pub fn window_count_through(&self,owner:Identity,window:u32,serial:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial<=serial).count()
    }
    /// Number of currently queued global events older than a serial.
    pub fn total_before_serial(&self,serial:u64)->usize{
        self.messages[..self.count].iter().flatten()
            .filter(|m|m.serial<serial).count()
    }
    /// Number of global events newer than the acknowledged serial.
    pub fn total_after_serial(&self,serial:u64)->usize{
        self.messages[..self.count].iter().flatten()
            .filter(|m|m.serial>serial).count()
    }
    /// Oldest message after one owner's observed serial.
    pub fn owner_first_after(&self,owner:Identity,serial:u64)->Option<Damage>{
        if owner.pid==0||owner.generation==0{return None;}
        self.messages[..self.count].iter().flatten().find(|m|
            m.owner==owner&&m.serial>serial).copied()
    }
    /// Newest message before one process's observed serial.
    pub fn owner_last_before(&self,owner:Identity,serial:u64)->Option<Damage>{
        if owner.pid==0||owner.generation==0{return None;}
        self.messages[..self.count].iter().rev().flatten().find(|m|
            m.owner==owner&&m.serial<serial).copied()
    }
    /// First message after a serial from the owner's given window.
    pub fn window_first_after(&self,owner:Identity,window:u32,serial:u64)
        ->Option<Damage>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().flatten().find(|m|
            m.owner==owner&&m.window==window&&m.serial>serial).copied()
    }
    /// Last message before a serial from an authorized owner window.
    pub fn window_last_before(&self,owner:Identity,window:u32,serial:u64)
        ->Option<Damage>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().rev().flatten().find(|m|
            m.owner==owner&&m.window==window&&m.serial<serial).copied()
    }
    /// Sum requested dirty surface area in this window after a serial.
    pub fn window_area_after(&self,owner:Identity,window:u32,serial:u64)->u64{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial>serial)
            .fold(0u64,|a,m|a.saturating_add(m.rect.w as u64*m.rect.h as u64))
    }
    /// Sum requested owner area until and including an observed serial.
    pub fn owner_area_through(&self,owner:Identity,serial:u64)->u64{
        if owner.pid==0||owner.generation==0{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.serial<=serial)
            .fold(0u64,|a,m|a.saturating_add(m.rect.w as u64*m.rect.h as u64))
    }
    /// Snapshot owner messages before a serial with no partial output.
    pub fn snapshot_owner_before(&self,owner:Identity,before:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0{return Err(Error::NotOwner);}
        let n=self.owner_pending_before(owner,before);
        if out.len()<n{return Err(Error::Full);}
        let mut i=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.serial<before{out[i]=*m;i+=1;}
        }
        Ok(i)
    }
    /// Snapshot exact-window updates newer than selected serial.
    pub fn snapshot_window_after(&self,owner:Identity,window:u32,after:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let n=self.window_count_after(owner,window,after);
        if out.len()<n{return Err(Error::Full);}
        let mut i=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.window==window&&m.serial>after{
                out[i]=*m;i+=1;
            }
        }
        Ok(i)
    }
    /// Snapshot same window updates <= serial without consuming them.
    pub fn snapshot_window_through(&self,owner:Identity,window:u32,serial:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let n=self.window_count_through(owner,window,serial);
        if out.len()<n{return Err(Error::Full);}
        let mut i=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.window==window&&m.serial<=serial{out[i]=*m;i+=1;}
        }
        Ok(i)
    }
    /// Drain matching window messages up to serial; preserve other PIDs.
    pub fn drain_window_through(&mut self,owner:Identity,window:u32,serial:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let required=self.window_count_through(owner,window,serial);
        if out.len()<required{return Err(Error::Full);}
        let mut i=0;let mut used=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&m.window==window&&
                m.serial<=serial){
                out[used]=self.messages[i].ok_or(Error::Empty)?;used+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(used)
    }
    /// Cancel all duplicate queued damage matching one exact rectangle.
    pub fn cancel_exact_rect(&mut self,owner:Identity,window:u32,rect:Rect)->usize{
        if owner.pid==0||owner.generation==0||window==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&
                m.window==window&&m.rect==rect){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// Cancel owner-window updates disjoint from a local region.
    pub fn cancel_disjoint_rect(&mut self,owner:Identity,window:u32,clip:Rect)->usize{
        if owner.pid==0||owner.generation==0||window==0||clip.w==0||clip.h==0{
            return 0;
        }
        let mut removed=0;let mut i=0;
        while i<self.count{
            let disjoint=self.messages[i].is_some_and(|m|{
                let r=m.rect;
                m.owner==owner&&m.window==window&&
                (r.x.saturating_add(r.w)<=clip.x||
                 clip.x.saturating_add(clip.w)<=r.x||
                 r.y.saturating_add(r.h)<=clip.y||
                 clip.y.saturating_add(clip.h)<=r.y)
            });
            if disjoint{
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    /// Add a requested rectangle to a live owner window only if idle.
    pub fn submit_if_window_idle<const W:usize>(&mut self,windows:&Windows<W>,
        owner:Identity,window:u32,rect:Rect)->Result<Option<u64>,Error>{
        if owner.pid==0||owner.generation==0||
            !windows.owns(owner.pid,owner.generation,window){
            return Err(Error::NotOwner);
        }
        if self.pending_window(owner,window)>0{return Ok(None);}
        self.submit(windows,owner,window,rect).map(Some)
    }
    /// Atomically submit only new owner window entries, disallow duplicates.
    pub fn submit_distinct_windows<const W:usize>(&mut self,windows:&Windows<W>,
        owner:Identity,changes:&[(u32,Rect)])->Result<usize,Error>{
        if changes.is_empty()||changes.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{messages:self.messages,count:self.count,next:self.next};
        for (i,&(window,rect)) in changes.iter().enumerate(){
            if changes[..i].iter().any(|(prior,_)|*prior==window){
                return Err(Error::Invalid);
            }
            staged.submit(windows,owner,window,rect)?;
        }
        *self=staged;Ok(changes.len())
    }
    /// Owner-scoped update cancellation on exact observed window serial.
    pub fn cancel_exact_window_epoch(&mut self,owner:Identity,window:u32,
        observed:u64)->bool{
        if !self.serial_is_window(owner,window,observed){return false;}
        self.cancel_window_serial(owner,window,observed)
    }
    /// Reap all queued frame updates from a retired owner PID incarnation.
    pub fn cancel_retired_incarnation(&mut self,owner:Identity)->usize{
        if owner.pid==0||owner.generation==0{return 0;}
        let count=self.pending_owner(owner);
        self.retire(owner);count
    }
}
#[cfg(test)]
mod r084_v5_surface_tests{
    use super::*;use crate::batch2_windows::Window;
    fn who()->Identity{Identity{pid:301,generation:1}}
    fn other()->Identity{Identity{pid:302,generation:1}}
    fn windows()->Windows<3>{
        let mut w=Windows::new();
        w.create(Window{pid:301,generation:1,id:1,
            rect:Rect{x:0,y:0,w:50,h:50}}).unwrap();
        w.create(Window{pid:301,generation:1,id:2,
            rect:Rect{x:55,y:0,w:50,h:50}}).unwrap();
        w.create(Window{pid:302,generation:1,id:3,
            rect:Rect{x:110,y:0,w:50,h:50}}).unwrap();w
    }
    fn rect()->Rect{Rect{x:2,y:2,w:3,h:4}}
    #[test]
    fn owner_ordering_and_frame_ownership_fail_closed(){
        let win=windows();let mut q=Surfaces::<8>::new();
        assert_eq!(q.submit_distinct_windows(&win,who(),&[(1,rect()),(2,rect())]),Ok(2));
        assert!(q.submit_if_window_idle(&win,who(),1,rect()).unwrap().is_none());
        assert_eq!(q.submit_if_window_idle(&win,other(),3,rect()),Ok(Some(3)));
        assert_eq!(q.window_count_after(who(),1,0),1);
        assert_eq!(q.window_count_through(who(),1,1),1);
        assert_eq!(q.total_before_serial(3),2);
        assert_eq!(q.total_after_serial(1),2);
        assert_eq!(q.owner_first_after(who(),1).unwrap().serial,2);
        assert_eq!(q.owner_last_before(who(),2).unwrap().serial,1);
        assert_eq!(q.window_first_after(who(),2,1).unwrap().serial,2);
        assert_eq!(q.window_last_before(who(),1,3).unwrap().serial,1);
        assert_eq!(q.window_area_after(who(),1,0),12);
        assert_eq!(q.owner_area_through(who(),2),24);
        let filler=q.peek_oldest().unwrap();let mut out=[filler;8];
        assert_eq!(q.snapshot_owner_before(who(),2,&mut out),Ok(1));
        assert_eq!(q.snapshot_window_after(who(),2,1,&mut out),Ok(1));
        assert_eq!(q.snapshot_window_through(who(),1,1,&mut out),Ok(1));
        assert_eq!(q.drain_window_through(who(),1,1,&mut out),Ok(1));
        assert_eq!(q.cancel_disjoint_rect(who(),2,Rect{x:40,y:40,w:1,h:1}),1);
        assert_eq!(q.cancel_exact_rect(other(),3,rect()),1);
        assert_eq!(q.pending_total(),0);
        assert!(!q.cancel_exact_window_epoch(who(),1,1));
        assert_eq!(q.cancel_retired_incarnation(who()),0);
        assert!(q.verify_damage_queue());
    }
}


// R084 batch VI: exact serial/window acknowledgement and geometry-aware
// damage selection. Every owner-facing method is tied to an Identity epoch.
impl<const N:usize> Surfaces<N>{
    // 1. Oldest pending serial owned by one process generation.
    pub fn first_owner_serial(&self,owner:Identity)->Option<u64>{
        self.peek_owner(owner).map(|m|m.serial)
    }
    // 2. Newest queued serial owned by the same process generation.
    pub fn last_owner_serial(&self,owner:Identity)->Option<u64>{
        self.newest_owner_damage(owner).map(|m|m.serial)
    }
    // 3. Number of exact owned-window updates inside an inclusive serial range.
    pub fn window_count_in_serial_range(&self,owner:Identity,window:u32,
        first:u64,last:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0||first==0||last<first{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial>=first&&m.serial<=last).count()
    }
    // 4. Sum actual queued rectangle area in an exact serial window.
    pub fn owner_area_in_serial_range(&self,owner:Identity,first:u64,last:u64)->u64{
        if owner.pid==0||owner.generation==0||first==0||last<first{return 0;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.serial>=first&&m.serial<=last)
            .fold(0u64,|a,m|a.saturating_add(m.rect.w as u64*m.rect.h as u64))
    }
    // 5. Count pending updates that intersect a selected local rectangle.
    pub fn window_intersections(&self,owner:Identity,window:u32,clip:Rect)->usize{
        self.overlapping_window_count(owner,window,clip)
    }
    // 6. Count owner's queued updates completely inside an input rectangle.
    pub fn window_contained_count(&self,owner:Identity,window:u32,clip:Rect)->usize{
        if owner.pid==0||owner.generation==0||window==0||clip.w==0||clip.h==0{return 0;}
        let Some(x2)=clip.x.checked_add(clip.w)else{return 0;};
        let Some(y2)=clip.y.checked_add(clip.h)else{return 0;};
        self.messages[..self.count].iter().flatten().filter(|m|{
            let r=m.rect;
            m.owner==owner&&m.window==window&&r.x>=clip.x&&r.y>=clip.y&&
                r.x.checked_add(r.w).is_some_and(|x|x<=x2)&&
                r.y.checked_add(r.h).is_some_and(|y|y<=y2)
        }).count()
    }
    // 7. Largest queued owner/window dirty rectangle by measured area.
    pub fn largest_window_damage(&self,owner:Identity,window:u32)->Option<Damage>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window)
            .max_by_key(|m|m.rect.w as u64*m.rect.h as u64).copied()
    }
    // 8. Smallest queued owner/window dirty rectangle by measured area.
    pub fn smallest_window_damage(&self,owner:Identity,window:u32)->Option<Damage>{
        if owner.pid==0||owner.generation==0||window==0{return None;}
        self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window)
            .min_by_key(|m|m.rect.w as u64*m.rect.h as u64).copied()
    }
    // 9. Atomically publish all owner-window updates after an observed epoch.
    pub fn snapshot_window_from_serial(&self,owner:Identity,window:u32,
        first:u64,out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0{return Err(Error::NotOwner);}
        let needed=self.messages[..self.count].iter().flatten().filter(|m|
            m.owner==owner&&m.window==window&&m.serial>=first).count();
        if out.len()<needed{return Err(Error::Full);}
        let mut n=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.window==window&&m.serial>=first{out[n]=*m;n+=1;}
        }
        Ok(n)
    }
    // 10. Snapshot exactly one owner's selected inclusive damage serial range.
    pub fn snapshot_owner_serial_range(&self,owner:Identity,first:u64,last:u64,
        out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||first==0||last<first{return Err(Error::Invalid);}
        let needed=self.owner_range_count(owner,first,last);
        if out.len()<needed{return Err(Error::Full);}
        let mut n=0;
        for m in self.messages[..self.count].iter().flatten(){
            if m.owner==owner&&m.serial>=first&&m.serial<=last{out[n]=*m;n+=1;}
        }
        Ok(n)
    }
    // 11. Exact owner/window acknowledgement through selected inclusive serial.
    pub fn acknowledge_window_through(&mut self,owner:Identity,window:u32,
        serial:u64)->usize{
        self.cancel_window_range(owner,window,1,serial)
    }
    // 12. Owner-only acknowledgement of a range, not peer queue entries.
    pub fn acknowledge_owner_range(&mut self,owner:Identity,from:u64,to:u64)->usize{
        self.cancel_owner_range(owner,from,to)
    }
    // 13. Remove only owner-window messages *outside* a selected serial range.
    pub fn retain_window_serial_range(&mut self,owner:Identity,window:u32,
        from:u64,to:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0||from==0||to<from{return 0;}
        let mut n=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|
                m.owner==owner&&m.window==window&&
                (m.serial<from||m.serial>to)){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;n+=1;
            }else{i+=1;}
        }
        n
    }
    // 14. Drain only owner/window updates in an exact inclusive range.
    pub fn drain_window_serial_range(&mut self,owner:Identity,window:u32,
        from:u64,to:u64,out:&mut[Damage])->Result<usize,Error>{
        if owner.pid==0||owner.generation==0||window==0||from==0||to<from{
            return Err(Error::Invalid);
        }
        let n=self.window_count_in_serial_range(owner,window,from,to);
        if out.len()<n{return Err(Error::Full);}
        let mut at=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|m.owner==owner&&
                m.window==window&&m.serial>=from&&m.serial<=to){
                out[at]=self.messages[i].ok_or(Error::Empty)?;at+=1;
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;
            }else{i+=1;}
        }
        Ok(at)
    }
    // 15. Cancel all pending exact rectangles for owner/window through serial.
    pub fn cancel_rect_through(&mut self,owner:Identity,window:u32,
        rect:Rect,through:u64)->usize{
        if owner.pid==0||owner.generation==0||window==0||through==0{return 0;}
        let mut removed=0;let mut i=0;
        while i<self.count{
            if self.messages[i].is_some_and(|m|
                m.owner==owner&&m.window==window&&m.rect==rect&&m.serial<=through){
                for j in i+1..self.count{self.messages[j-1]=self.messages[j];}
                self.count-=1;self.messages[self.count]=None;removed+=1;
            }else{i+=1;}
        }
        removed
    }
    // 16. Replace an exact serial rectangle only when its old bounds match.
    pub fn compare_replace_damage<const W:usize>(&mut self,windows:&Windows<W>,
        owner:Identity,window:u32,serial:u64,previous:Rect,next:Rect)
        ->Result<(),Error>{
        self.replace_rect_if_equal(windows,owner,window,serial,previous,next)
    }
    // 17. Guard a damage update by current window geometry, no clipping.
    pub fn window_can_accept_damage<const W:usize>(&self,windows:&Windows<W>,
        owner:Identity,window:u32,rect:Rect)->bool{
        if owner.pid==0||owner.generation==0||window==0||
            !windows.owns(owner.pid,owner.generation,window){return false;}
        let Some(r)=windows.get(window).map(|w|w.rect)else{return false;};
        rect.w!=0&&rect.h!=0&&rect.x.checked_add(rect.w).is_some_and(|v|v<=r.w)&&
            rect.y.checked_add(rect.h).is_some_and(|v|v<=r.h)
    }
    // 18. Batch check geometry and queue capacity before any publication.
    pub fn can_submit_distinct_batch<const W:usize>(&self,windows:&Windows<W>,
        owner:Identity,items:&[(u32,Rect)])->bool{
        if items.is_empty()||items.len()>self.free_message_slots(){return false;}
        if self.next.checked_add(items.len() as u64).is_none(){return false;}
        for (i,&(window,rect)) in items.iter().enumerate(){
            if items[..i].iter().any(|(seen,_)|*seen==window)||
                !self.window_can_accept_damage(windows,owner,window,rect){return false;}
        }
        true
    }
    // 19. Publish only preflighted distinct-window updates, all-or-nothing.
    pub fn submit_validated_distinct<const W:usize>(&mut self,windows:&Windows<W>,
        owner:Identity,items:&[(u32,Rect)])->Result<usize,Error>{
        if !self.can_submit_distinct_batch(windows,owner,items){return Err(Error::Invalid);}
        self.submit_distinct_windows(windows,owner,items)
    }
    // 20. Verify current damage entries all refer to owned live windows.
    pub fn validate_live_queue<const W:usize>(&self,windows:&Windows<W>)->bool{
        self.verify_damage_queue()&&self.messages[..self.count].iter().flatten().all(|m|
            self.window_can_accept_damage(windows,m.owner,m.window,m.rect))
    }
}
#[cfg(test)]
mod r084_batch_vi_compositor_tests{
    use super::*;use crate::batch2_windows::Window;
    fn owner()->Identity{Identity{pid:541,generation:1}}
    fn peer()->Identity{Identity{pid:542,generation:1}}
    fn windows()->Windows<3>{
        let mut w=Windows::new();
        w.create(Window{pid:541,generation:1,id:1,
            rect:Rect{x:0,y:0,w:30,h:30}}).unwrap();
        w.create(Window{pid:541,generation:1,id:2,
            rect:Rect{x:40,y:0,w:30,h:30}}).unwrap();
        w.create(Window{pid:542,generation:1,id:3,
            rect:Rect{x:80,y:0,w:30,h:30}}).unwrap();w
    }
    fn rect()->Rect{Rect{x:2,y:3,w:4,h:5}}
    #[test]
    fn serial_range_selection_and_atomic_drain_keep_foreign_damage(){
        let windows=windows();let mut q=Surfaces::<8>::new();
        assert!(q.can_submit_distinct_batch(&windows,owner(),&[(1,rect()),(2,rect())]));
        assert_eq!(q.submit_validated_distinct(&windows,owner(),&[(1,rect()),(2,rect())]),Ok(2));
        let other=q.submit(&windows,peer(),3,rect()).unwrap();
        assert_eq!(q.first_owner_serial(owner()),Some(1));
        assert_eq!(q.last_owner_serial(owner()),Some(2));
        assert_eq!(q.window_count_in_serial_range(owner(),1,1,3),1);
        assert_eq!(q.owner_area_in_serial_range(owner(),1,2),40);
        assert_eq!(q.window_intersections(owner(),1,rect()),1);
        assert_eq!(q.window_contained_count(owner(),1,Rect{x:0,y:0,w:10,h:10}),1);
        assert_eq!(q.largest_window_damage(owner(),1).unwrap().serial,1);
        assert_eq!(q.smallest_window_damage(owner(),1).unwrap().serial,1);
        assert!(q.window_can_accept_damage(&windows,owner(),1,rect()));
        assert!(q.validate_live_queue(&windows));
        let filler=q.peek_oldest().unwrap();let mut empty:[Damage;0]=[];
        assert_eq!(q.snapshot_owner_serial_range(owner(),1,3,&mut empty),Err(Error::Full));
        let mut out=[filler;8];
        assert_eq!(q.snapshot_window_from_serial(owner(),1,1,&mut out),Ok(1));
        assert_eq!(q.snapshot_owner_serial_range(owner(),1,2,&mut out),Ok(2));
        assert_eq!(q.compare_replace_damage(&windows,owner(),1,1,rect(),
            Rect{x:3,y:4,w:5,h:6}),Ok(()));
        assert_eq!(q.drain_window_serial_range(owner(),1,1,2,&mut out),Ok(1));
        assert_eq!(q.retain_window_serial_range(owner(),2,2,2),0);
        assert_eq!(q.acknowledge_window_through(owner(),2,2),1);
        assert_eq!(q.acknowledge_owner_range(peer(),other,other),1);
        assert_eq!(q.cancel_rect_through(owner(),1,rect(),20),0);
        assert_eq!(q.pending_total(),0);
        assert!(q.validate_live_queue(&windows));
    }
}
