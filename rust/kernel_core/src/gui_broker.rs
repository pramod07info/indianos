//! R029.1: allocator-free per-process window grants and focus event mailbox.
//! The kernel supplies each PID after validating the running process. Neither
//! a window ID nor a userspace buffer is an authority to choose an owner.
pub const FOCUS:u32=1;
pub const POINTER_FOCUS:u32=2;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{Invalid,Duplicate,Full,Unauthorized,Empty,Exhausted}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
struct Binding{owner:u32,window:u32}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Event{pub owner:u32,pub window:u32,pub kind:u32,pub sequence:u32}
impl Event{
    pub fn wire(self)->[u8;16]{
        let mut b=[0u8;16];
        b[..4].copy_from_slice(&self.sequence.to_le_bytes());
        b[4..8].copy_from_slice(&self.window.to_le_bytes());
        b[8..12].copy_from_slice(&self.kind.to_le_bytes());
        b
    }
}
/// Single-CPU syscall context only: no IRQ or SMP concurrency allowed.
pub struct Broker<const W:usize,const Q:usize>{
    windows:[Option<Binding>;W],events:[Option<Event>;Q],len:usize,sequence:u32
}
impl<const W:usize,const Q:usize> Broker<W,Q>{
    pub const fn new()->Self{
        Self{windows:[None;W],events:[None;Q],len:0,sequence:0}
    }
    /// Grants are performed only by the trusted window service.
    pub fn grant(&mut self,owner:u32,window:u32)->Result<(),Error>{
        if owner==0||window==0{return Err(Error::Invalid);}
        if self.windows.iter().flatten().any(|b|b.window==window){
            return Err(Error::Duplicate);
        }
        let slot=self.windows.iter_mut().find(|b|b.is_none()).ok_or(Error::Full)?;
        *slot=Some(Binding{owner,window});Ok(())
    }
    pub fn owns(&self,owner:u32,window:u32)->bool{
        owner!=0&&window!=0&&self.windows.iter().flatten()
            .any(|b|b.owner==owner&&b.window==window)
    }
    pub fn owner(&self,window:u32)->Option<u32>{
        if window==0{return None;}
        self.windows.iter().flatten().find(|b|b.window==window).map(|b|b.owner)
    }
    pub fn known(&self,owner:u32)->bool{
        owner!=0&&self.windows.iter().flatten().any(|b|b.owner==owner)
    }
    pub fn can_post(&self,owner:u32,window:u32)->bool{
        self.owns(owner,window)&&self.len<Q&&self.sequence!=u32::MAX
    }
    fn post(&mut self,owner:u32,window:u32,kind:u32)->Result<Event,Error>{
        if !self.owns(owner,window){return Err(Error::Unauthorized);}
        if self.len>=Q{return Err(Error::Full);}
        let sequence=self.sequence.checked_add(1).ok_or(Error::Exhausted)?;
        let event=Event{owner,window,kind,sequence};
        self.events[self.len]=Some(event);self.len+=1;self.sequence=sequence;
        Ok(event)
    }
    pub fn post_focus(&mut self,owner:u32,window:u32)->Result<Event,Error>{
        self.post(owner,window,FOCUS)
    }
    pub fn post_pointer_focus(&mut self,owner:u32,window:u32)->Result<Event,Error>{
        self.post(owner,window,POINTER_FOCUS)
    }
    pub fn peek(&self,owner:u32)->Result<Event,Error>{
        if !self.known(owner){return Err(Error::Unauthorized);}
        self.events[..self.len].iter().flatten()
            .find(|e|e.owner==owner).copied().ok_or(Error::Empty)
    }
    pub fn ack(&mut self,owner:u32,sequence:u32)->Result<(),Error>{
        if !self.known(owner){return Err(Error::Unauthorized);}
        let index=self.events[..self.len].iter().position(|e|
            e.is_some_and(|e|e.owner==owner&&e.sequence==sequence))
            .ok_or(Error::Empty)?;
        self.remove(index);Ok(())
    }
    /// Orbit L5: transfer all pending notifications for an authorized PID
    /// as one batch. A short output slice cannot partially consume events.
    /// Per-owner FIFO is preserved; events for other processes stay queued.
    pub fn drain_owner(&mut self,owner:u32,into:&mut[Event])
        ->Result<usize,Error>{
        if !self.known(owner){return Err(Error::Unauthorized);}
        let needed=self.events[..self.len].iter().flatten()
            .filter(|e|e.owner==owner).count();
        if needed>into.len(){return Err(Error::Full);}
        let mut n=0;
        let mut i=0;
        while i<self.len{
            if let Some(event)=self.events[i]{
                if event.owner==owner{
                    into[n]=event;n+=1;self.remove(i);continue;
                }
            }
            i+=1;
        }
        Ok(n)
    }
    /// Atomically publish a sequence of owner-bound GUI events. This is
    /// serialized kernel input context; SMP locking is not implemented.
    pub fn post_batch(&mut self,owner:u32,events:&[(u32,u32)])
        ->Result<usize,Error>{
        if events.is_empty(){return Err(Error::Invalid);}
        if owner==0{return Err(Error::Unauthorized);}
        if self.len.checked_add(events.len()).is_none_or(|n|n>Q){
            return Err(Error::Full);
        }
        if self.sequence.checked_add(events.len() as u32).is_none(){
            return Err(Error::Exhausted);
        }
        for &(window,kind) in events{
            if kind!=FOCUS&&kind!=POINTER_FOCUS{return Err(Error::Invalid);}
            if !self.owns(owner,window){return Err(Error::Unauthorized);}
        }
        for &(window,kind) in events{
            self.post(owner,window,kind)?;
        }
        Ok(events.len())
    }
    /// Non-destructive status on a trusted owner queue. Other owners are
    /// not disclosed to clients.
    pub fn pending_owner(&self,owner:u32)->Result<usize,Error>{
        if !self.known(owner){return Err(Error::Unauthorized);}
        Ok(self.events[..self.len].iter().flatten()
            .filter(|e|e.owner==owner).count())
    }
    /// Drop a *single* withdrawn owner/window grant along with only the
    /// queued events targeted at that window.
    pub fn revoke_window(&mut self,owner:u32,window:u32)->Result<(),Error>{
        let grant=self.windows.iter_mut().find(|g|
            g.is_some_and(|x|x.owner==owner&&x.window==window))
            .ok_or(Error::Unauthorized)?;
        *grant=None;
        let mut i=0;
        while i<self.len{
            if self.events[i].is_some_and(|e|e.owner==owner&&e.window==window){
                self.remove(i);
            }else{i+=1;}
        }
        Ok(())
    }
    /// Preview latest event for one authenticated owner; never consumes
    /// messages or advances sequence numbers.
    pub fn latest_owner(&self,owner:u32)->Result<Event,Error>{
        if !self.known(owner){return Err(Error::Unauthorized);}
        self.events[..self.len].iter().rev().flatten()
            .find(|e|e.owner==owner).copied().ok_or(Error::Empty)
    }
    fn remove(&mut self,index:usize){
        for i in index..self.len-1{self.events[i]=self.events[i+1];}
        self.len-=1;self.events[self.len]=None;
    }
    /// Clear both grants and queued input after process teardown.
    pub fn revoke(&mut self,owner:u32){
        if owner==0{return;}
        for w in &mut self.windows{
            if w.is_some_and(|b|b.owner==owner){*w=None;}
        }
        let mut i=0;
        while i<self.len{
            if self.events[i].is_some_and(|e|e.owner==owner){self.remove(i);}
            else{i+=1;}
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn orbit_event_batches_never_publish_partial_foreign_events(){
        let mut b=Broker::<3,4>::new();b.grant(1,10).unwrap();
        b.grant(1,11).unwrap();b.grant(2,12).unwrap();
        assert_eq!(b.post_batch(1,&[(10,FOCUS),(12,FOCUS)]),
                   Err(Error::Unauthorized));
        assert_eq!(b.pending_owner(1),Ok(0));
        assert_eq!(b.post_batch(1,&[(10,FOCUS),(11,POINTER_FOCUS)]),
                   Ok(2));
        assert_eq!(b.pending_owner(1),Ok(2));
        assert_eq!(b.latest_owner(1).unwrap().window,11);
    }
    #[test]fn orbit_queue_capacity_and_grant_revocation_preserve_other_events(){
        let mut b=Broker::<3,3>::new();b.grant(1,10).unwrap();
        b.grant(1,11).unwrap();b.grant(2,12).unwrap();
        b.post_batch(1,&[(10,FOCUS),(11,POINTER_FOCUS)]).unwrap();
        assert_eq!(b.post_batch(2,&[(12,FOCUS),(12,FOCUS)]),
                   Err(Error::Full));
        assert_eq!(b.pending_owner(2),Ok(0));
        b.post_focus(2,12).unwrap();
        b.revoke_window(1,10).unwrap();
        assert_eq!(b.pending_owner(1),Ok(1));
        assert_eq!(b.pending_owner(2),Ok(1));
        assert!(!b.owns(1,10));
        assert_eq!(b.post_batch(1,&[(10,FOCUS)]),Err(Error::Unauthorized));
    }
    #[test]fn orbit_notification_drain_is_atomic_fifo_and_owner_bound(){
        let mut broker=Broker::<2,5>::new();
        broker.grant(7,1).unwrap();broker.grant(8,2).unwrap();
        let first=broker.post_focus(7,1).unwrap();
        let peer=broker.post_pointer_focus(8,2).unwrap();
        let last=broker.post_pointer_focus(7,1).unwrap();
        let mut short=[Event{owner:0,window:0,kind:0,sequence:0};1];
        assert_eq!(broker.drain_owner(7,&mut short),Err(Error::Full));
        assert_eq!(broker.peek(7),Ok(first));
        let mut out=[Event{owner:0,window:0,kind:0,sequence:0};2];
        assert_eq!(broker.drain_owner(7,&mut out),Ok(2));
        assert_eq!(out,[first,last]);
        assert_eq!(broker.peek(8),Ok(peer));
        assert_eq!(broker.peek(7),Err(Error::Empty));
        assert_eq!(broker.drain_owner(9,&mut out),Err(Error::Unauthorized));
    }
    #[test]fn cross_pid_spoof_and_duplicate_grants_denied(){
        let mut b=Broker::<2,3>::new();
        assert_eq!(b.grant(0,1),Err(Error::Invalid));
        assert_eq!(b.grant(1,0),Err(Error::Invalid));
        b.grant(1,1).unwrap();b.grant(2,2).unwrap();
        assert_eq!(b.grant(2,1),Err(Error::Duplicate));
        assert_eq!(b.grant(3,3),Err(Error::Full));
        assert!(b.owns(1,1));assert!(!b.owns(1,2));
        assert!(!b.owns(2,1));assert!(!b.owns(0,1));
        assert_eq!(b.post_focus(2,1),Err(Error::Unauthorized));
    }
    #[test]fn fifo_ack_preserves_other_pid_events_and_rejects_full(){
        let mut b=Broker::<2,2>::new();
        b.grant(1,1).unwrap();b.grant(2,2).unwrap();
        let a=b.post_focus(1,1).unwrap();
        let c=b.post_focus(2,2).unwrap();
        assert_eq!(b.post_focus(1,1),Err(Error::Full));
        assert_eq!(b.ack(2,a.sequence),Err(Error::Empty));
        assert_eq!(b.peek(1),Ok(a));
        assert_eq!(b.peek(2),Ok(c));
        b.ack(1,a.sequence).unwrap();
        assert_eq!(b.peek(1),Err(Error::Empty));
        assert_eq!(b.peek(2),Ok(c));
        assert_eq!(b.post_focus(1,1).unwrap().sequence,3);
    }
    #[test]fn process_reap_drops_its_events_only(){
        let mut b=Broker::<3,4>::new();
        b.grant(1,1).unwrap();b.grant(2,2).unwrap();b.grant(1,3).unwrap();
        b.post_focus(1,1).unwrap();let survivor=b.post_focus(2,2).unwrap();
        b.post_focus(1,3).unwrap();
        b.revoke(1);
        assert!(!b.known(1));assert_eq!(b.peek(1),Err(Error::Unauthorized));
        assert_eq!(b.peek(2),Ok(survivor));b.ack(2,survivor.sequence).unwrap();
        b.grant(2,1).unwrap();
        assert_eq!(b.peek(2),Err(Error::Empty));
    }
    #[test]fn physical_input_resolves_authenticated_owner_and_reaps(){
        let mut b=Broker::<2,4>::new();
        b.grant(1,1).unwrap();b.grant(2,2).unwrap();
        assert_eq!(b.owner(2),Some(2));
        assert_eq!(b.owner(0),None);
        assert_eq!(b.post_pointer_focus(1,2),Err(Error::Unauthorized));
        let ev=b.post_pointer_focus(2,2).unwrap();
        assert_eq!(ev.kind,POINTER_FOCUS);
        assert_eq!(ev.wire()[8..12],[2,0,0,0]);
        b.revoke(2);
        assert_eq!(b.owner(2),None);
        assert_eq!(b.peek(2),Err(Error::Unauthorized));
    }
    #[test]fn wire_empty_queue_zero_capacity_and_overflow(){
        let mut b=Broker::<1,1>::new();b.grant(1,2).unwrap();
        assert_eq!(b.peek(1),Err(Error::Empty));
        let event=b.post_focus(1,2).unwrap();
        assert_eq!(event.wire(),[1,0,0,0,2,0,0,0,1,0,0,0,0,0,0,0]);
        b.ack(1,event.sequence).unwrap();
        b.sequence=u32::MAX;
        assert!(!b.can_post(1,2));
        assert_eq!(b.post_focus(1,2),Err(Error::Exhausted));
        let mut z=Broker::<0,0>::new();
        assert_eq!(z.grant(1,1),Err(Error::Full));
    }
}


/// R033: route physical pointer focus using the kernel's window-owner map,
/// never an untrusted caller-supplied PID. This remains a broker primitive;
/// a full ring-3 compositor and QEMU input wiring are separate acceptance.
impl<const W: usize, const Q: usize> Broker<W, Q> {
    pub fn route_physical_focus(&mut self, trusted_window: u32)
        -> Result<Event, Error>
    {
        let owner = self.owner(trusted_window).ok_or(Error::Unauthorized)?;
        self.post_pointer_focus(owner, trusted_window)
    }

    /// Check focus permissions against the process authenticated by syscall
    /// dispatch. Never infer identity from the window or event payload.
    pub fn route_client_focus(&mut self, authenticated_pid: u32, window: u32)
        -> Result<Event, Error>
    {
        if !self.owns(authenticated_pid, window) { return Err(Error::Unauthorized); }
        self.post_focus(authenticated_pid, window)
    }

    /// A faulted user's grants and stale events are dropped while peer events
    /// continue to be accessible; no new window state is synthesized here.
    pub fn faulted(&mut self, authenticated_pid: u32) {
        self.revoke(authenticated_pid);
    }
}
#[cfg(test)]
mod r033_focus_tests {
    use super::*;
    #[test]
    fn physical_window_target_uses_trusted_owner_not_supplied_pid() {
        let mut b = Broker::<2, 4>::new();
        b.grant(10, 100).unwrap();
        b.grant(20, 200).unwrap();
        let ev = b.route_physical_focus(200).unwrap();
        assert_eq!((ev.owner, ev.window, ev.kind), (20, 200, POINTER_FOCUS));
        assert_eq!(b.peek(10), Err(Error::Empty));
        assert_eq!(b.peek(20), Ok(ev));
        assert_eq!(b.route_physical_focus(999), Err(Error::Unauthorized));
    }
    #[test]
    fn untrusted_client_cannot_move_focus_to_foreign_window() {
        let mut b = Broker::<2, 4>::new();
        b.grant(10, 1).unwrap();
        b.grant(20, 2).unwrap();
        assert_eq!(b.route_client_focus(10, 2), Err(Error::Unauthorized));
        let ev = b.route_client_focus(10, 1).unwrap();
        assert_eq!(ev.owner, 10);
    }
    #[test]
    fn fault_disposes_grants_and_events_without_affecting_peer() {
        let mut b = Broker::<2, 4>::new();
        b.grant(10, 1).unwrap();
        b.grant(20, 2).unwrap();
        b.route_physical_focus(1).unwrap();
        let other = b.route_physical_focus(2).unwrap();
        b.faulted(10);
        assert_eq!(b.route_physical_focus(1), Err(Error::Unauthorized));
        assert_eq!(b.peek(10), Err(Error::Unauthorized));
        assert_eq!(b.peek(20), Ok(other));
        assert_eq!(b.route_physical_focus(2).unwrap().sequence, 3);
    }
}


/// Batch 1: geometry belongs to an authenticated process and is never
/// moved by a peer. Coordinates are checked before a move is committed.
/// A separate userspace compositor still needs to consume these grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedSurface {
    pub owner: u32, pub window: u32,
    pub rectangle: crate::agent_05_surface_rect::Rectangle,
}
pub struct SurfaceLayout<const N: usize> {
    surfaces: [Option<OwnedSurface>; N],
}
impl<const N: usize> SurfaceLayout<N> {
    pub const fn new() -> Self { Self { surfaces: [None; N] } }
    pub fn grant(&mut self, owner: u32, window: u32,
        rectangle: crate::agent_05_surface_rect::Rectangle) -> Result<(), Error> {
        if owner == 0 || window == 0 { return Err(Error::Invalid); }
        if self.surfaces.iter().flatten().any(|s|s.window == window) {
            return Err(Error::Duplicate);
        }
        crate::agent_05_surface_rect::clip_owned(rectangle,rectangle,owner,owner)
            .map_err(|_| Error::Invalid)?;
        let free = self.surfaces.iter_mut().find(|slot| slot.is_none())
            .ok_or(Error::Full)?;
        *free=Some(OwnedSurface{owner,window,rectangle});
        Ok(())
    }
    pub fn geometry(&self, owner: u32, window: u32)
        -> Result<crate::agent_05_surface_rect::Rectangle, Error> {
        self.surfaces.iter().flatten().find(|s|s.owner == owner &&
            s.window == window && owner != 0)
            .map(|s|s.rectangle).ok_or(Error::Unauthorized)
    }
    pub fn move_owned(&mut self, owner: u32, window: u32, dx: i32, dy: i32)
        -> Result<crate::agent_05_surface_rect::Rectangle, Error> {
        let slot = self.surfaces.iter_mut().flatten()
            .find(|s|s.owner == owner && s.window == window && owner != 0)
            .ok_or(Error::Unauthorized)?;
        let moved=crate::agent_05_surface_rect::move_owned(
            slot.rectangle, dx, dy, owner, owner).map_err(|_|Error::Invalid)?;
        slot.rectangle=moved;
        Ok(moved)
    }
    pub fn revoke(&mut self, owner: u32) {
        if owner == 0 { return; }
        for slot in &mut self.surfaces {
            if slot.is_some_and(|s|s.owner == owner){*slot=None;}
        }
    }
}
#[cfg(test)]
mod batch1_surface_layout_tests {
    use super::*;
    use crate::agent_05_surface_rect::Rectangle;
    #[test]
    fn only_owner_moves_surface_and_invalid_move_keeps_state() {
        let mut layout=SurfaceLayout::<2>::new();
        let original=Rectangle{x:20,y:30,width:100,height:50};
        layout.grant(10,100,original).unwrap();
        assert_eq!(layout.move_owned(20,100,1,1),Err(Error::Unauthorized));
        assert_eq!(layout.move_owned(10,100,-21,0),Err(Error::Invalid));
        assert_eq!(layout.geometry(10,100),Ok(original));
        assert_eq!(layout.move_owned(10,100,5,-5).unwrap().x,25);
    }
    #[test]
    fn teardown_prevents_reused_window_from_inheriting_geometry() {
        let mut layout=SurfaceLayout::<2>::new();
        let r=Rectangle{x:0,y:0,width:10,height:10};
        layout.grant(1,100,r).unwrap();
        assert_eq!(layout.grant(2,100,r),Err(Error::Duplicate));
        layout.revoke(1);
        assert_eq!(layout.geometry(1,100),Err(Error::Unauthorized));
        layout.grant(2,100,r).unwrap();
        assert_eq!(layout.geometry(2,100),Ok(r));
    }
}



/// Orbit R005: transactional window ownership lifecycle in serialized kernel
/// context. Actual ring-3 compositor wiring and SMP protection are separate.
impl<const W:usize,const Q:usize> Broker<W,Q>{
 /// Register an entire workspace only when every window has valid,
 /// nonduplicated ownership. Partial application is forbidden.
 pub fn grant_batch(&mut self,requests:&[(u32,u32)])->Result<usize,Error>{
  if requests.is_empty()||requests.len()>W{return Err(Error::Invalid);}
  let mut pending=Self{windows:self.windows,events:self.events,
   len:self.len,sequence:self.sequence};
  for &(owner,window) in requests{pending.grant(owner,window)?;}
  self.windows=pending.windows;Ok(requests.len())
 }
 /// Fully tear down several owner/window bindings or none at all.
 pub fn revoke_batch(&mut self,requests:&[(u32,u32)])->Result<usize,Error>{
  if requests.is_empty()||requests.len()>W{return Err(Error::Invalid);}
  let mut pending=Self{windows:self.windows,events:self.events,
   len:self.len,sequence:self.sequence};
  for &(owner,window) in requests{pending.revoke_window(owner,window)?;}
  self.windows=pending.windows;self.events=pending.events;
  self.len=pending.len;Ok(requests.len())
 }
 /// Trusted window service can reassign a window after an app restart.
 /// Buffered input from the old process is scrubbed, never inherited.
 pub fn reassign_window(&mut self,old_owner:u32,new_owner:u32,window:u32)
  ->Result<(),Error>{
  if old_owner==0||new_owner==0||old_owner==new_owner||window==0{
   return Err(Error::Invalid);
  }
  let index=self.windows.iter().position(|w|
   w.is_some_and(|b|b.owner==old_owner&&b.window==window))
   .ok_or(Error::Unauthorized)?;
  // Forget queued events before publishing the new owner grant.
  let mut pos=0;
  while pos<self.len{
   if self.events[pos].is_some_and(|e|e.window==window){self.remove(pos);}
   else{pos+=1;}
  }
  self.windows[index]=Some(Binding{owner:new_owner,window});
  Ok(())
 }
}
#[cfg(test)]
mod orbit_r005_broker_tests{
 use super::*;
 #[test]fn workspace_grants_are_all_or_nothing(){
  let mut b=Broker::<3,4>::new();
  assert_eq!(b.grant_batch(&[(7,10),(7,11),(8,12)]),Ok(3));
  assert_eq!(b.grant_batch(&[(9,13),(9,11)]),Err(Error::Full));
  assert!(!b.owns(9,13));assert!(b.owns(8,12));
 }
 #[test]fn transfer_rejects_stale_events_and_does_not_hijack_other_windows(){
  let mut b=Broker::<3,4>::new();b.grant(1,10).unwrap();b.grant(2,20).unwrap();
  b.post_focus(1,10).unwrap();let peer=b.post_focus(2,20).unwrap();
  assert_eq!(b.reassign_window(2,3,10),Err(Error::Unauthorized));
  b.reassign_window(1,3,10).unwrap();
  assert_eq!(b.peek(3),Err(Error::Empty));
  assert_eq!(b.peek(2),Ok(peer));
  assert_eq!(b.post_focus(1,10),Err(Error::Unauthorized));
  assert_eq!(b.post_focus(3,10).unwrap().owner,3);
 }
 #[test]fn invalid_revocation_batch_cannot_destroy_any_grants(){
  let mut b=Broker::<3,4>::new();b.grant_batch(&[(1,10),(1,11),(2,20)]).unwrap();
  let event=b.post_focus(1,10).unwrap();
  assert_eq!(b.revoke_batch(&[(1,10),(2,999)]),Err(Error::Unauthorized));
  assert_eq!(b.peek(1),Ok(event));assert!(b.owns(1,10));
  assert_eq!(b.revoke_batch(&[(1,10),(1,11)]),Ok(2));
  assert!(!b.known(1));assert!(b.known(2));
 }
}

 
/// R009 input delivery protocol. The trusted input driver may queue decoded
/// keys only against a window owned by the authenticated receiving process.
/// Event.kind carries the key and press/release information without changing
/// the existing fixed-size 16-byte mailbox wire layout.
pub const KEY_DOWN:u32=3;
pub const KEY_UP:u32=4;
pub const POINTER_CLICK:u32=5;
pub fn decode_key_event(kind:u32)->Option<(u16,bool)>{
 let tag=kind&0xffff;
 let code=(kind>>16) as u16;
 if code==0||code>255{return None;}
 match tag{KEY_DOWN=>Some((code,true)),KEY_UP=>Some((code,false)),_=>None}
}
impl<const W:usize,const Q:usize> Broker<W,Q>{
 pub fn post_keyboard(&mut self,authenticated_owner:u32,window:u32,
   scan_code:u16,pressed:bool)->Result<Event,Error>{
  if scan_code==0||scan_code>255{return Err(Error::Invalid);}
  let tag=if pressed{KEY_DOWN}else{KEY_UP};
  self.post(authenticated_owner,window,((scan_code as u32)<<16)|tag)
 }
 pub fn post_pointer_click(&mut self,authenticated_owner:u32,window:u32,
   button:u8)->Result<Event,Error>{
  if button==0||button>3{return Err(Error::Invalid);}
  self.post(authenticated_owner,window,((button as u32)<<16)|POINTER_CLICK)
 }
 /// Deliver a batch of decoded keys atomically: no partial publication
 /// if ownership, capacity or sequence space checks fail.
 pub fn post_keyboard_batch(&mut self,authenticated_owner:u32,window:u32,
   keys:&[(u16,bool)])->Result<usize,Error>{
  if keys.is_empty()||keys.len()>Q{return Err(Error::Invalid);}
  if !self.owns(authenticated_owner,window){return Err(Error::Unauthorized);}
  if self.len.checked_add(keys.len()).is_none_or(|n|n>Q){return Err(Error::Full);}
  if self.sequence.checked_add(keys.len() as u32).is_none(){return Err(Error::Exhausted);}
  if keys.iter().any(|(k,_)|*k==0||*k>255){return Err(Error::Invalid);}
  for &(key,down) in keys{self.post_keyboard(authenticated_owner,window,key,down)?;}
  Ok(keys.len())
 }
}
#[cfg(test)]
mod r009_input_delivery_tests{
 use super::*;
 #[test]fn keyboard_event_wire_is_owned_and_decodable(){
  let mut broker=Broker::<2,5>::new();
  broker.grant(12,40).unwrap();broker.grant(13,41).unwrap();
  let e=broker.post_keyboard(12,40,30,true).unwrap();
  assert_eq!(decode_key_event(e.kind),Some((30,true)));
  assert_eq!(u32::from_le_bytes(e.wire()[8..12].try_into().unwrap()),e.kind);
  assert_eq!(broker.post_keyboard(13,40,30,false),Err(Error::Unauthorized));
  assert_eq!(broker.post_pointer_click(12,40,0),Err(Error::Invalid));
 }
 #[test]fn batch_preflight_prevents_partial_key_delivery(){
  let mut broker=Broker::<1,2>::new();broker.grant(17,2).unwrap();
  assert_eq!(broker.post_keyboard_batch(17,2,&[(1,true),(0,false)]),Err(Error::Invalid));
  assert_eq!(broker.pending_owner(17),Ok(0));
  assert_eq!(broker.post_keyboard_batch(17,2,&[(1,true),(1,false)]),Ok(2));
  assert_eq!(broker.post_keyboard_batch(17,2,&[(1,true)]),Err(Error::Full));
  let mut events=[Event{owner:0,window:0,kind:0,sequence:0};2];
  assert_eq!(broker.drain_owner(17,&mut events),Ok(2));
  assert_eq!(events[0].sequence+1,events[1].sequence);
 }
 #[test]fn process_exit_revokes_owned_input_without_touching_peer(){
  let mut broker=Broker::<2,3>::new();
  broker.grant(5,9).unwrap();broker.grant(6,10).unwrap();
  broker.post_keyboard(5,9,8,true).unwrap();
  broker.post_keyboard(6,10,9,true).unwrap();
  broker.revoke(5);
  assert_eq!(broker.peek(5),Err(Error::Unauthorized));
  assert_eq!(decode_key_event(broker.peek(6).unwrap().kind),Some((9,true)));
 }
}

 
/// R010 single-compositor input routing. The active window is selected only
/// by a trusted compositor, never by a client-supplied target PID.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FocusTarget{pub owner:u32,pub window:u32}
pub struct DesktopInput{
 focused:Option<FocusTarget>,capture:Option<FocusTarget>,
}
impl DesktopInput{
 pub const fn new()->Self{Self{focused:None,capture:None}}
 pub fn focused(&self)->Option<FocusTarget>{self.focused}
 pub fn captured(&self)->Option<FocusTarget>{self.capture}
 pub fn focus<const W:usize,const Q:usize>(&mut self,broker:&mut Broker<W,Q>,
  window:u32)->Result<FocusTarget,Error>{
  let owner=broker.owner(window).ok_or(Error::Unauthorized)?;
  // Notify before changing state: a full input queue must not steal focus.
  broker.post_focus(owner,window)?;
  let target=FocusTarget{owner,window};self.focused=Some(target);
  self.capture=None;Ok(target)
 }
 /// Capture starts only from the focused, authenticated owner.
 pub fn capture_pointer<const W:usize,const Q:usize>(&mut self,
  broker:&Broker<W,Q>,owner:u32,window:u32)->Result<(),Error>{
  let target=FocusTarget{owner,window};
  if self.focused!=Some(target)||!broker.owns(owner,window){
   return Err(Error::Unauthorized);
  }
  self.capture=Some(target);Ok(())
 }
 pub fn release_pointer(&mut self,owner:u32){
  if self.capture.is_some_and(|t|t.owner==owner){self.capture=None;}
 }
 pub fn keyboard<const W:usize,const Q:usize>(&mut self,
  broker:&mut Broker<W,Q>,code:u16,pressed:bool)->Result<Event,Error>{
  let target=self.focused.ok_or(Error::Empty)?;
  if !broker.owns(target.owner,target.window){
   self.focused=None;self.capture=None;return Err(Error::Unauthorized);
  }
  broker.post_keyboard(target.owner,target.window,code,pressed)
 }
 pub fn click<const W:usize,const Q:usize>(&mut self,
  broker:&mut Broker<W,Q>,button:u8)->Result<Event,Error>{
  let target=self.capture.or(self.focused).ok_or(Error::Empty)?;
  if !broker.owns(target.owner,target.window){
   self.capture=None;self.focused=None;return Err(Error::Unauthorized);
  }
  broker.post_pointer_click(target.owner,target.window,button)
 }
 pub fn process_exited<const W:usize,const Q:usize>(&mut self,
  broker:&mut Broker<W,Q>,owner:u32){
  broker.revoke(owner);
  if self.focused.is_some_and(|t|t.owner==owner){self.focused=None;}
  if self.capture.is_some_and(|t|t.owner==owner){self.capture=None;}
 }
}
#[cfg(test)]
mod r010_focus_capture_tests{
 use super::*;
 #[test]fn physical_focus_routes_to_authenticated_window_owner(){
  let mut broker=Broker::<2,8>::new();broker.grant(11,101).unwrap();broker.grant(22,202).unwrap();
  let mut input=DesktopInput::new();
  assert_eq!(input.focus(&mut broker,202),Ok(FocusTarget{owner:22,window:202}));
  assert_eq!(input.keyboard(&mut broker,30,true).unwrap().owner,22);
  assert_eq!(input.focus(&mut broker,999),Err(Error::Unauthorized));
  assert_eq!(input.focused(),Some(FocusTarget{owner:22,window:202}));
 }
 #[test]fn pointer_capture_cannot_be_stolen_by_foreign_window(){
  let mut broker=Broker::<2,8>::new();broker.grant(11,101).unwrap();broker.grant(22,202).unwrap();
  let mut input=DesktopInput::new();input.focus(&mut broker,101).unwrap();
  assert_eq!(input.capture_pointer(&broker,22,202),Err(Error::Unauthorized));
  input.capture_pointer(&broker,11,101).unwrap();
  assert_eq!(input.click(&mut broker,1).unwrap().window,101);
  input.release_pointer(22);
  assert_eq!(input.captured(),Some(FocusTarget{owner:11,window:101}));
  input.release_pointer(11);assert_eq!(input.captured(),None);
 }
 #[test]fn crashing_owner_releases_focus_capture_and_queued_events(){
  let mut broker=Broker::<2,8>::new();broker.grant(11,101).unwrap();broker.grant(22,202).unwrap();
  let mut input=DesktopInput::new();input.focus(&mut broker,101).unwrap();
  input.capture_pointer(&broker,11,101).unwrap();
  input.process_exited(&mut broker,11);
  assert_eq!(input.focused(),None);assert_eq!(input.captured(),None);
  assert_eq!(input.keyboard(&mut broker,1,true),Err(Error::Empty));
  assert_eq!(input.focus(&mut broker,202).unwrap().owner,22);
 }
}

 
/// R011 generation-checked compositor work queue. A trusted surface broker
/// supplies ownership; no window ID in a client event grants draw authority.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FrameDamage{pub owner:u32,pub window:u32,pub epoch:u64,
 pub x:u32,pub y:u32,pub width:u32,pub height:u32}
pub struct DamageQueue<const N:usize>{
 entries:[Option<FrameDamage>;N],len:usize,next_epoch:u64,
}
impl<const N:usize> DamageQueue<N>{
 pub const fn new()->Self{Self{entries:[None;N],len:0,next_epoch:1}}
 pub fn pending(&self)->usize{self.len}
 pub fn invalidate_owner(&mut self,owner:u32)->usize{
  let mut out=0;
  for i in 0..self.len{
   if let Some(d)=self.entries[i]{
    if d.owner==owner{out+=1;self.entries[i]=None;}
   }
  }
  if out>0{
   let mut write=0;
   for read in 0..self.len{
    if let Some(d)=self.entries[read]{self.entries[write]=Some(d);write+=1;}
   }
   for i in write..self.len{self.entries[i]=None;}
   self.len=write;
  }
  out
 }
 /// Validate whole batch, including window ownership and rectangles, before
 /// publishing any frame updates.
 pub fn submit<const W:usize>(&mut self,layout:&SurfaceLayout<W>,
  owner:u32,requests:&[(u32,u32,u32,u32,u32)])->Result<u64,Error>{
  if owner==0||requests.is_empty()||requests.len()>N.saturating_sub(self.len){
   return Err(Error::Full);
  }
  let sequence=self.next_epoch.checked_add(1).ok_or(Error::Exhausted)?;
  for &(window,x,y,w,h) in requests{
   let geometry=layout.geometry(owner,window)?;
   if w==0||h==0||x.checked_add(w).is_none()||
      y.checked_add(h).is_none()||
      x.checked_add(w).is_none_or(|end|end>geometry.width)||
      y.checked_add(h).is_none_or(|end|end>geometry.height){
    return Err(Error::Invalid);
   }
  }
  for &(window,x,y,w,h) in requests{
   self.entries[self.len]=Some(FrameDamage{owner,window,epoch:sequence,
      x,y,width:w,height:h});
   self.len+=1;
  }
  self.next_epoch=sequence;Ok(sequence)
 }
 /// Fail-atomic drain: caller provides all required output slots and each
 /// output entry has the same monotonically assigned frame epoch.
 pub fn drain(&mut self,out:&mut[FrameDamage])->Result<usize,Error>{
  if out.len()<self.len{return Err(Error::Full);}
  let n=self.len;
  for (i,slot) in self.entries[..n].iter().enumerate(){
   out[i]=slot.ok_or(Error::Invalid)?;
  }
  self.entries[..n].fill(None);self.len=0;Ok(n)
 }
}
#[cfg(test)]
mod r011_damage_queue_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn layout()->SurfaceLayout<3>{
  let mut l=SurfaceLayout::<3>::new();
  l.grant(3,10,Rectangle{x:0,y:0,width:100,height:80}).unwrap();
  l.grant(4,20,Rectangle{x:100,y:40,width:50,height:40}).unwrap();l
 }
 #[test]fn redraw_batch_is_all_or_nothing_and_epoch_ordered(){
  let l=layout();let mut q=DamageQueue::<3>::new();
  assert_eq!(q.submit(&l,3,&[(10,0,0,10,10),(20,0,0,10,10)]),
    Err(Error::Unauthorized));
  assert_eq!(q.pending(),0);
  let first=q.submit(&l,3,&[(10,0,0,10,10),(10,60,50,30,20)]).unwrap();
  assert_eq!(q.submit(&l,3,&[(10,100,0,1,1)]),Err(Error::Invalid));
  let mut out=[FrameDamage{owner:0,window:0,epoch:0,
    x:0,y:0,width:0,height:0};3];
  assert_eq!(q.drain(&mut out),Ok(2));
  assert_eq!(out[0].epoch,first);assert_eq!(out[1].epoch,first);
  assert_eq!(q.pending(),0);
 }
 #[test]fn short_output_does_not_consume_pending_frames(){
  let l=layout();let mut q=DamageQueue::<2>::new();
  q.submit(&l,3,&[(10,0,0,5,5),(10,10,10,5,5)]).unwrap();
  let mut short=[FrameDamage{owner:0,window:0,epoch:0,
    x:0,y:0,width:0,height:0};1];
  assert_eq!(q.drain(&mut short),Err(Error::Full));
  assert_eq!(q.pending(),2);
 }
 #[test]fn crashed_client_cannot_erase_other_clients_frame(){
  let l=layout();let mut q=DamageQueue::<3>::new();
  q.submit(&l,3,&[(10,0,0,5,5)]).unwrap();
  q.submit(&l,4,&[(20,1,1,5,5)]).unwrap();
  assert_eq!(q.invalidate_owner(3),1);
  assert_eq!(q.pending(),1);
  let mut out=[FrameDamage{owner:0,window:0,epoch:0,
    x:0,y:0,width:0,height:0};1];
  assert_eq!(q.drain(&mut out),Ok(1));assert_eq!(out[0].owner,4);
 }
}
