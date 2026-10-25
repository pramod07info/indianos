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

