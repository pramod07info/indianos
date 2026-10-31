//! R029.1: allocator-free per-process window grants and focus event mailbox.
//! The kernel supplies each PID after validating the running process. Neither
//! a window ID nor a userspace buffer is an authority to choose an owner.
pub const FOCUS:u32=1;
pub const POINTER_FOCUS:u32=2;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{Invalid,Duplicate,Full,Unauthorized,Empty,Exhausted}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
// Epochs distinguish successive grants of the same owner/window pair.
struct Binding{owner:u32,window:u32,epoch:u64}
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
    windows:[Option<Binding>;W],events:[Option<Event>;Q],len:usize,sequence:u32,grant_epoch:u64
}
impl<const W:usize,const Q:usize> Broker<W,Q>{
    pub const fn new()->Self{
        Self{windows:[None;W],events:[None;Q],len:0,sequence:0,grant_epoch:0}
    }
    /// Grants are performed only by the trusted window service.
    pub fn grant(&mut self,owner:u32,window:u32)->Result<(),Error>{
        if owner==0||window==0{return Err(Error::Invalid);}
        if self.windows.iter().flatten().any(|b|b.window==window){
            return Err(Error::Duplicate);
        }
        let slot=self.windows.iter_mut().find(|b|b.is_none()).ok_or(Error::Full)?;
        let epoch=self.grant_epoch.checked_add(1).ok_or(Error::Exhausted)?;
        *slot=Some(Binding{owner,window,epoch});self.grant_epoch=epoch;Ok(())
    }
    fn binding(&self,owner:u32,window:u32)->Option<Binding>{
        self.windows.iter().flatten().find(|b|b.owner==owner&&b.window==window).copied()
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
    surfaces: [Option<OwnedSurface>; N], epochs:[u64;N], next_epoch:u64,
}
impl<const N: usize> SurfaceLayout<N> {
    pub const fn new() -> Self { Self { surfaces: [None; N], epochs:[0;N], next_epoch:0 } }
    pub fn grant(&mut self, owner: u32, window: u32,
        rectangle: crate::agent_05_surface_rect::Rectangle) -> Result<(), Error> {
        if owner == 0 || window == 0 { return Err(Error::Invalid); }
        if self.surfaces.iter().flatten().any(|s|s.window == window) {
            return Err(Error::Duplicate);
        }
        crate::agent_05_surface_rect::clip_owned(rectangle,rectangle,owner,owner)
            .map_err(|_| Error::Invalid)?;
        let index = self.surfaces.iter().position(|slot| slot.is_none())
            .ok_or(Error::Full)?;
        let epoch=self.next_epoch.checked_add(1).ok_or(Error::Exhausted)?;
        self.surfaces[index]=Some(OwnedSurface{owner,window,rectangle});
        self.epochs[index]=epoch;
        self.next_epoch=epoch;
        Ok(())
    }
    fn epoch(&self,owner:u32,window:u32)->Option<u64>{
        self.surfaces.iter().position(|s|s.is_some_and(|s|
            s.owner==owner&&s.window==window)).map(|i|self.epochs[i])
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
        if moved!=slot.rectangle{
            let epoch=self.next_epoch.checked_add(1).ok_or(Error::Exhausted)?;
            slot.rectangle=moved;
            let index=self.surfaces.iter().position(|s|s.is_some_and(|s|
                s.owner==owner&&s.window==window)).unwrap();
            self.epochs[index]=epoch;self.next_epoch=epoch;
        }
        Ok(moved)
    }
    /// Move several authenticated surfaces in one serialized desktop
    /// transaction. Validate every owner, window, geometry and epoch
    /// through a private shadow layout; failures cannot partially move
    /// windows or advance any published surface grant epoch.
    pub fn move_batch(&mut self,requests:&[(u32,u32,i32,i32)])
        ->Result<usize,Error>{
        if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{surfaces:self.surfaces,epochs:self.epochs,
            next_epoch:self.next_epoch};
        for (i,&(owner,window,dx,dy)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|&(prior_owner,prior_window,_,_)|
               owner==prior_owner&&window==prior_window){
                return Err(Error::Duplicate);
            }
            staged.move_owned(owner,window,dx,dy)?;
        }
        self.surfaces=staged.surfaces;
        self.epochs=staged.epochs;
        self.next_epoch=staged.next_epoch;
        Ok(requests.len())
    }
    /// Reposition and resize a collection of trusted application surfaces
    /// in one transaction. Every target must be fully inside the desktop
    /// and owned by its original process; duplicate windows are forbidden.
    /// A failed late validation does not change ANY geometry or epoch.
    /// A no-op resize intentionally does not generate redraw epochs.
    pub fn set_geometry_batch(&mut self,
        requests:&[(u32,u32,crate::agent_05_surface_rect::Rectangle)]
    )->Result<usize,Error>{
        if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
        let mut staged=Self{surfaces:self.surfaces,epochs:self.epochs,
            next_epoch:self.next_epoch};
        for (i,&(owner,window,rectangle)) in requests.iter().enumerate(){
            if requests[..i].iter().any(|&(_,prior,_)|prior==window){
                return Err(Error::Duplicate);
            }
            let index=staged.surfaces.iter().position(|slot|
                slot.is_some_and(|s|s.owner==owner&&s.window==window))
                .ok_or(Error::Unauthorized)?;
            crate::agent_05_surface_rect::clip_owned(
                rectangle,rectangle,owner,owner).map_err(|_|Error::Invalid)?;
            let previous=staged.surfaces[index].ok_or(Error::Unauthorized)?;
            if previous.rectangle!=rectangle{
                let epoch=staged.next_epoch.checked_add(1).ok_or(Error::Exhausted)?;
                staged.surfaces[index]=Some(OwnedSurface{owner,window,rectangle});
                staged.epochs[index]=epoch;
                staged.next_epoch=epoch;
            }
        }
        self.surfaces=staged.surfaces;
        self.epochs=staged.epochs;
        self.next_epoch=staged.next_epoch;
        Ok(requests.len())
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
    fn r058_batch_window_move_commits_every_surface_and_epoch(){
        let mut layout=SurfaceLayout::<3>::new();
        let first=Rectangle{x:5,y:7,width:40,height:20};
        let second=Rectangle{x:70,y:4,width:20,height:10};
        layout.grant(5,100,first).unwrap();
        layout.grant(6,200,second).unwrap();
        let previous_a=layout.epoch(5,100).unwrap();
        let previous_b=layout.epoch(6,200).unwrap();
        assert_eq!(layout.move_batch(&[(5,100,10,2),(6,200,-5,8)]),Ok(2));
        assert_eq!(layout.geometry(5,100).unwrap().x,15);
        assert_eq!(layout.geometry(5,100).unwrap().y,9);
        assert_eq!(layout.geometry(6,200).unwrap().x,65);
        assert_eq!(layout.geometry(6,200).unwrap().y,12);
        assert!(layout.epoch(5,100).unwrap()>previous_a);
        assert!(layout.epoch(6,200).unwrap()>previous_b);
    }
    #[test]
    fn r058_invalid_batch_preserves_all_window_coordinates_and_epochs(){
        let mut layout=SurfaceLayout::<3>::new();
        let first=Rectangle{x:5,y:5,width:40,height:20};
        let second=Rectangle{x:20,y:20,width:20,height:10};
        layout.grant(5,100,first).unwrap();layout.grant(6,200,second).unwrap();
        let before_a=layout.epoch(5,100);
        let before_b=layout.epoch(6,200);
        assert_eq!(layout.move_batch(&[(5,100,2,2),(9,200,1,1)]),
                   Err(Error::Unauthorized));
        assert_eq!(layout.move_batch(&[(5,100,2,2),(6,200,-100,0)]),
                   Err(Error::Invalid));
        assert_eq!(layout.move_batch(&[(5,100,1,1),(5,100,1,1)]),
                   Err(Error::Duplicate));
        assert_eq!(layout.move_batch(&[]),Err(Error::Invalid));
        assert_eq!(layout.geometry(5,100),Ok(first));
        assert_eq!(layout.geometry(6,200),Ok(second));
        assert_eq!(layout.epoch(5,100),before_a);
        assert_eq!(layout.epoch(6,200),before_b);
    }
    #[test]
    fn r059_atomic_batch_resize_and_reposition_updates_owned_surface_epochs(){
        let mut layout=SurfaceLayout::<3>::new();
        let old=Rectangle{x:5,y:10,width:30,height:25};
        let peer=Rectangle{x:50,y:50,width:40,height:35};
        layout.grant(1,10,old).unwrap();layout.grant(2,20,peer).unwrap();
        let old_epoch=layout.epoch(1,10).unwrap();
        let peer_epoch=layout.epoch(2,20).unwrap();
        let bigger=Rectangle{x:9,y:15,width:90,height:55};
        assert_eq!(layout.set_geometry_batch(&[(1,10,bigger),(2,20,peer)]),Ok(2));
        assert_eq!(layout.geometry(1,10),Ok(bigger));
        assert_eq!(layout.geometry(2,20),Ok(peer));
        assert!(layout.epoch(1,10).unwrap()>old_epoch);
        assert_eq!(layout.epoch(2,20),Some(peer_epoch));
        let current=layout.epoch(1,10);
        assert_eq!(layout.set_geometry_batch(&[(1,10,bigger)]),Ok(1));
        assert_eq!(layout.epoch(1,10),current);
    }
    #[test]
    fn r059_invalid_second_resize_rolls_back_first_window_and_epochs(){
        let mut layout=SurfaceLayout::<3>::new();
        let a=Rectangle{x:10,y:10,width:40,height:30};
        let b=Rectangle{x:80,y:60,width:20,height:20};
        layout.grant(3,30,a).unwrap();layout.grant(4,40,b).unwrap();
        let old_a=layout.epoch(3,30);let old_b=layout.epoch(4,40);
        let valid=Rectangle{x:10,y:10,width:60,height:45};
        let bad=Rectangle{x:1270,y:4,width:20,height:20};
        assert_eq!(layout.set_geometry_batch(&[(3,30,valid),(4,40,bad)]),
                   Err(Error::Invalid));
        assert_eq!(layout.set_geometry_batch(&[(3,30,valid),(9,40,b)]),
                   Err(Error::Unauthorized));
        assert_eq!(layout.set_geometry_batch(&[(3,30,valid),(3,30,a)]),
                   Err(Error::Duplicate));
        assert_eq!(layout.set_geometry_batch(&[]),Err(Error::Invalid));
        assert_eq!(layout.geometry(3,30),Ok(a));
        assert_eq!(layout.geometry(4,40),Ok(b));
        assert_eq!(layout.epoch(3,30),old_a);
        assert_eq!(layout.epoch(4,40),old_b);
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
   len:self.len,sequence:self.sequence,grant_epoch:self.grant_epoch};
  for &(owner,window) in requests{pending.grant(owner,window)?;}
  self.windows=pending.windows;self.grant_epoch=pending.grant_epoch;Ok(requests.len())
 }
 /// Fully tear down several owner/window bindings or none at all.
 pub fn revoke_batch(&mut self,requests:&[(u32,u32)])->Result<usize,Error>{
  if requests.is_empty()||requests.len()>W{return Err(Error::Invalid);}
  let mut pending=Self{windows:self.windows,events:self.events,
   len:self.len,sequence:self.sequence,grant_epoch:self.grant_epoch};
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
  let epoch=self.grant_epoch.checked_add(1).ok_or(Error::Exhausted)?;
  // Forget queued events before publishing the new owner grant.
  let mut pos=0;
  while pos<self.len{
   if self.events[pos].is_some_and(|e|e.window==window){self.remove(pos);}
   else{pos+=1;}
  }
  self.windows[index]=Some(Binding{owner:new_owner,window,epoch});
  self.grant_epoch=epoch;
  Ok(())
 }
 /// Transfer several crashed-app windows to replacement owners as one
 /// transaction. Previously queued input for every old binding is erased;
 /// unrelated windows, event order and global sequence are untouched.
 /// The coordinator must retire old owner resources before publishing
 /// any new compositor surfaces, and must serialize this with input IRQs.
 pub fn reassign_batch(&mut self,requests:&[(u32,u32,u32)])
  ->Result<usize,Error>{
  if requests.is_empty()||requests.len()>W{return Err(Error::Invalid);}
  let mut pending=Self{windows:self.windows,events:self.events,
   len:self.len,sequence:self.sequence,grant_epoch:self.grant_epoch};
  for (index,&(old_owner,new_owner,window)) in requests.iter().enumerate(){
   if requests[..index].iter().any(|&(_,_,prior_window)|prior_window==window){
    return Err(Error::Duplicate);
   }
   pending.reassign_window(old_owner,new_owner,window)?;
  }
  self.windows=pending.windows;
  self.events=pending.events;
  self.len=pending.len;
  self.grant_epoch=pending.grant_epoch;
  Ok(requests.len())
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
 #[test]fn m1_r056_multi_window_recovery_erases_old_input_but_preserves_peer(){
  let mut b=Broker::<4,8>::new();
  b.grant_batch(&[(3,10),(3,11),(4,20)]).unwrap();
  b.post_focus(3,10).unwrap();
  b.post_pointer_focus(3,11).unwrap();
  let peer=b.post_focus(4,20).unwrap();
  assert_eq!(b.reassign_batch(&[(3,5,10),(3,5,11)]),Ok(2));
  assert_eq!(b.owner(10),Some(5));
  assert_eq!(b.owner(11),Some(5));
  assert_eq!(b.pending_owner(5),Ok(0));
  assert_eq!(b.pending_owner(4),Ok(1));
  assert_eq!(b.peek(4),Ok(peer));
  assert_eq!(b.post_focus(3,10),Err(Error::Unauthorized));
  assert!(b.post_focus(5,10).unwrap().sequence>peer.sequence);
 }
 #[test]fn m1_r056_invalid_transfer_batch_never_reassigns_partial_windows(){
  let mut b=Broker::<4,8>::new();
  b.grant_batch(&[(3,10),(3,11),(4,20)]).unwrap();
  let old=b.post_focus(3,10).unwrap();
  for changes in [
   [(3,5,10),(4,5,11)],
   [(3,5,10),(3,6,10)],
   [(3,5,10),(3,5,999)]
  ]{
   assert!(b.reassign_batch(&changes).is_err());
   assert_eq!(b.owner(10),Some(3));
   assert_eq!(b.owner(11),Some(3));
   assert_eq!(b.peek(3),Ok(old));
   assert!(!b.known(5));
  }
  assert_eq!(b.reassign_batch(&[]),Err(Error::Invalid));
  assert_eq!(b.post_pointer_focus(3,10).unwrap().sequence,old.sequence+1);
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
 focused:Option<Binding>,capture:Option<Binding>,
}
impl DesktopInput{
 pub const fn new()->Self{Self{focused:None,capture:None}}
 pub fn focused(&self)->Option<FocusTarget>{self.focused.map(|b|FocusTarget{owner:b.owner,window:b.window})}
 pub fn captured(&self)->Option<FocusTarget>{self.capture.map(|b|FocusTarget{owner:b.owner,window:b.window})}
 pub fn focus<const W:usize,const Q:usize>(&mut self,broker:&mut Broker<W,Q>,
  window:u32)->Result<FocusTarget,Error>{
  let owner=broker.owner(window).ok_or(Error::Unauthorized)?;
  // Notify before changing state: a full input queue must not steal focus.
  broker.post_focus(owner,window)?;
  let target=FocusTarget{owner,window};self.focused=broker.binding(owner,window);
  self.capture=None;Ok(target)
 }
 /// Capture starts only from the focused, authenticated owner.
 pub fn capture_pointer<const W:usize,const Q:usize>(&mut self,
  broker:&Broker<W,Q>,owner:u32,window:u32)->Result<(),Error>{
  let target=broker.binding(owner,window).ok_or(Error::Unauthorized)?;
  if self.focused!=Some(target){
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
  if broker.binding(target.owner,target.window)!=Some(target){
   self.focused=None;self.capture=None;return Err(Error::Unauthorized);
  }
  broker.post_keyboard(target.owner,target.window,code,pressed)
 }
 pub fn click<const W:usize,const Q:usize>(&mut self,
  broker:&mut Broker<W,Q>,button:u8)->Result<Event,Error>{
  let target=self.capture.or(self.focused).ok_or(Error::Empty)?;
  if broker.binding(target.owner,target.window)!=Some(target){
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
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct DamageAuthority{grant:u64,surface:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct QueuedDamage{frame:FrameDamage,authority:Option<DamageAuthority>}
impl core::ops::Deref for QueuedDamage{
 type Target=FrameDamage;
 fn deref(&self)->&FrameDamage{&self.frame}
}
pub struct DamageQueue<const N:usize>{
 entries:[Option<QueuedDamage>;N],len:usize,next_epoch:u64,
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
   self.entries[self.len]=Some(QueuedDamage{frame:FrameDamage{owner,window,epoch:sequence,
      x,y,width:w,height:h},authority:None});
   self.len+=1;
  }
  self.next_epoch=sequence;Ok(sequence)
 }
 /// Bind a whole validated frame to the current broker and surface grants.
 /// Legacy submit() remains usable for trusted local queues, but its work is
 /// deliberately ineligible for present_fair_verified(). Serialized context
 /// must retain these same Broker/SurfaceLayout instances for their lifetime.
 pub fn submit_verified<const B:usize,const Q:usize,const L:usize>(
  &mut self,broker:&Broker<B,Q>,layout:&SurfaceLayout<L>,owner:u32,
  requests:&[(u32,u32,u32,u32,u32)])->Result<u64,Error>{
  if owner==0||requests.is_empty(){return Err(Error::Invalid);}
  for &(window,_,_,_,_) in requests{
   broker.binding(owner,window).ok_or(Error::Unauthorized)?;
  }
  let start=self.len;
  let epoch=self.submit(layout,owner,requests)?;
  for queued in self.entries[start..self.len].iter_mut().flatten(){
   let grant=broker.binding(owner,queued.window).unwrap().epoch;
   let surface=layout.epoch(owner,queued.window).unwrap();
   queued.authority=Some(DamageAuthority{grant,surface});
  }
  Ok(epoch)
 }
 /// Fail-atomic drain: caller provides all required output slots and each
 /// output entry has the same monotonically assigned frame epoch.
 pub fn drain(&mut self,out:&mut[FrameDamage])->Result<usize,Error>{
  if out.len()<self.len{return Err(Error::Full);}
  let n=self.len;
  for (i,slot) in self.entries[..n].iter().enumerate(){
   out[i]=slot.ok_or(Error::Invalid)?.frame;
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


/// R012: compositor acknowledgement is separate from fetching draw work.
/// Failed or untrusted presentation must not discard pending damage.
impl<const N:usize> DamageQueue<N>{
 pub fn preview_owner(&self,owner:u32,out:&mut[FrameDamage])
  ->Result<usize,Error>{
  if owner==0{return Err(Error::Unauthorized);}
  let required=self.entries[..self.len].iter().flatten()
    .filter(|d|d.owner==owner).count();
  if out.len()<required{return Err(Error::Full);}
  let mut copied=0;
  for damage in self.entries[..self.len].iter().flatten(){
   if damage.owner==owner{out[copied]=damage.frame;copied+=1;}
  }
  Ok(copied)
 }
 /// A trusted compositor calls this after a successful presentation fence.
 /// Only exact owner/epoch records are removed, preserving other windows.
 pub fn acknowledge(&mut self,owner:u32,epoch:u64)
  ->Result<usize,Error>{
  if owner==0||epoch==0{return Err(Error::Unauthorized);}
  if !self.entries[..self.len].iter().flatten().any(|d|
     d.owner==owner&&d.epoch==epoch){return Err(Error::Empty);}
  let mut write=0;let mut removed=0;
  for read in 0..self.len{
   if let Some(d)=self.entries[read]{
    if d.owner==owner&&d.epoch==epoch{removed+=1;}
    else{self.entries[write]=Some(d);write+=1;}
   }
  }
  self.entries[write..self.len].fill(None);self.len=write;
  Ok(removed)
 }
 pub fn pending_owner(&self,owner:u32)->usize{
  self.entries[..self.len].iter().flatten().filter(|d|d.owner==owner).count()
 }
}
#[cfg(test)]
mod r012_damage_presentation_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn layout()->SurfaceLayout<2>{
  let mut l=SurfaceLayout::new();
  l.grant(1,10,Rectangle{x:0,y:0,width:80,height:60}).unwrap();
  l.grant(2,20,Rectangle{x:0,y:0,width:80,height:60}).unwrap();l
 }
 fn empty()->FrameDamage{FrameDamage{owner:0,window:0,epoch:0,
  x:0,y:0,width:0,height:0}}
 #[test]fn preview_does_not_consume_and_ack_releases_only_frame(){
  let layout=layout();let mut q=DamageQueue::<4>::new();
  let a=q.submit(&layout,1,&[(10,0,0,1,1),(10,3,3,2,2)]).unwrap();
  let b=q.submit(&layout,2,&[(20,1,1,3,3)]).unwrap();
  let mut out=[empty();2];
  assert_eq!(q.preview_owner(1,&mut out),Ok(2));
  assert_eq!(out[0].epoch,a);
  assert_eq!(q.pending(),3);
  assert_eq!(q.acknowledge(1,a),Ok(2));
  assert_eq!(q.pending_owner(2),1);
  assert_eq!(q.acknowledge(2,b),Ok(1));
 }
 #[test]fn rejected_wrong_epoch_and_short_buffer_do_not_mutate(){
  let layout=layout();let mut q=DamageQueue::<2>::new();
  let epoch=q.submit(&layout,1,&[(10,0,0,2,2),(10,2,2,1,1)]).unwrap();
  let mut out=[empty();1];
  assert_eq!(q.preview_owner(1,&mut out),Err(Error::Full));
  assert_eq!(out,[empty()]);
  assert_eq!(q.acknowledge(2,epoch),Err(Error::Empty));
  assert_eq!(q.pending(),2);
 }
 #[test]fn removing_crashed_owner_preserves_others_ack_queue(){
  let layout=layout();let mut q=DamageQueue::<2>::new();
  q.submit(&layout,1,&[(10,0,0,1,1)]).unwrap();
  let epoch=q.submit(&layout,2,&[(20,0,0,1,1)]).unwrap();
  assert_eq!(q.invalidate_owner(1),1);
  assert_eq!(q.acknowledge(2,epoch),Ok(1));
  assert_eq!(q.pending(),0);
 }
}

/// R013 fair one-owner-at-a-time compositor presentation. An owner with
/// many queued frames cannot permanently starve a different owner.
impl<const N:usize> DamageQueue<N>{
 pub fn present_fair(&mut self,last_owner:Option<u32>,out:&mut[FrameDamage])
   ->Result<(u32,u64,usize),Error>{
  let first=self.entries[..self.len].iter().flatten().next().copied()
    .ok_or(Error::Empty)?;
  let chosen=self.entries[..self.len].iter().flatten()
    .find(|event|Some(event.owner)!=last_owner)
    .copied().unwrap_or(first);
  let needed=self.entries[..self.len].iter().flatten().filter(|e|
    e.owner==chosen.owner&&e.epoch==chosen.epoch).count();
  if out.len()<needed{return Err(Error::Full);}
  let mut written=0;
  for event in self.entries[..self.len].iter().flatten(){
   if event.owner==chosen.owner&&event.epoch==chosen.epoch{
    out[written]=event.frame;written+=1;
   }
  }
  let retired=self.acknowledge(chosen.owner,chosen.epoch)?;
  Ok((chosen.owner,chosen.epoch,retired))
 }
 pub fn fault_owner<const W:usize,const Q:usize>(
  &mut self,broker:&mut Broker<W,Q>,owner:u32)->usize{
  broker.revoke(owner);
  self.invalidate_owner(owner)
 }
 pub fn distinct_owners(&self)->usize{
  let mut count=0;
  for i in 0..self.len{
   if let Some(event)=self.entries[i]{
    if !self.entries[..i].iter().flatten().any(|e|e.owner==event.owner){
     count+=1;
    }
   }
  }
  count
 }
}
#[cfg(test)]
mod r013_fair_frames_tests{
 use super::*;use crate::agent_05_surface_rect::Rectangle;
 fn layout()->SurfaceLayout<2>{
  let mut l=SurfaceLayout::new();
  l.grant(1,10,Rectangle{x:0,y:0,width:40,height:30}).unwrap();
  l.grant(2,20,Rectangle{x:0,y:0,width:40,height:30}).unwrap();l
 }
 fn blank()->FrameDamage{FrameDamage{owner:0,window:0,epoch:0,
  x:0,y:0,width:0,height:0}}
 #[test]fn round_robin_owner_switches_with_backlog(){
  let l=layout();let mut q=DamageQueue::<4>::new();
  q.submit(&l,1,&[(10,0,0,2,2)]).unwrap();
  q.submit(&l,1,&[(10,3,3,2,2)]).unwrap();
  q.submit(&l,2,&[(20,0,0,1,1)]).unwrap();
  assert_eq!(q.distinct_owners(),2);
  let mut out=[blank();2];
  assert_eq!(q.present_fair(None,&mut out).unwrap().0,1);
  assert_eq!(q.present_fair(Some(1),&mut out).unwrap().0,2);
  assert_eq!(q.present_fair(Some(2),&mut out).unwrap().0,1);
  assert_eq!(q.pending(),0);
 }
 #[test]fn undersized_output_never_consumes_frames(){
  let l=layout();let mut q=DamageQueue::<3>::new();
  q.submit(&l,1,&[(10,0,0,2,2),(10,3,3,2,2)]).unwrap();
  let mut out=[blank();1];
  assert_eq!(q.present_fair(None,&mut out),Err(Error::Full));
  assert_eq!(q.pending(),2);assert_eq!(out,[blank()]);
 }
 #[test]fn crashing_owner_preserves_peer_frames(){
  let l=layout();let mut q=DamageQueue::<2>::new();
  let mut broker=Broker::<2,4>::new();
  broker.grant(1,10).unwrap();broker.grant(2,20).unwrap();
  q.submit(&l,1,&[(10,0,0,2,2)]).unwrap();
  q.submit(&l,2,&[(20,0,0,2,2)]).unwrap();
  assert_eq!(q.fault_owner(&mut broker,1),1);
  assert_eq!(q.pending(),1);
  assert_eq!(q.present_fair(None,&mut[blank()]).unwrap().0,2);
 }
}

/// R014 compositor-level bounded dirty rectangle coalescing, preserving
/// the original frame epoch while combining adjacent app updates.
impl<const N:usize> DamageQueue<N>{
 pub fn combine_window_damage(&mut self,owner:u32,window:u32)
   ->Result<usize,Error>{
  if owner==0||window==0{return Err(Error::Invalid);}
  let mut count=0;let mut minx=u32::MAX;let mut miny=u32::MAX;
  let mut maxx=0u32;let mut maxy=0u32;let mut epoch=0u64;
  for event in self.entries[..self.len].iter().flatten(){
   if event.owner==owner&&event.window==window{
    count+=1;minx=minx.min(event.x);miny=miny.min(event.y);
    maxx=maxx.max(event.x.checked_add(event.width).ok_or(Error::Invalid)?);
    maxy=maxy.max(event.y.checked_add(event.height).ok_or(Error::Invalid)?);
    epoch=if epoch==0{event.epoch}else{epoch.min(event.epoch)};
   }
  }
  if count<2{return Ok(0);}
  let authority=self.entries[..self.len].iter().flatten()
   .find(|e|e.owner==owner&&e.window==window).unwrap().authority;
  // Never merge work across authority incarnations: doing so can launder
  // stale damage into a newly authorized frame.
  if self.entries[..self.len].iter().flatten().any(|e|
   e.owner==owner&&e.window==window&&e.authority!=authority){
   return Err(Error::Unauthorized);
  }
  let combined=QueuedDamage{frame:FrameDamage{owner,window,epoch,x:minx,y:miny,
    width:maxx-minx,height:maxy-miny},authority};
  let mut remaining=[None;N];let mut len=0;let mut inserted=false;
  for event in self.entries[..self.len].iter().flatten(){
   if event.owner==owner&&event.window==window{
    if !inserted{remaining[len]=Some(combined);len+=1;inserted=true;}
   }else{remaining[len]=Some(*event);len+=1;}
  }
  self.entries=remaining;self.len=len;Ok(count-1)
 }
 /// A compositor must never replay damage whose owner/window has been
 /// revoked or whose bounds changed. Keep peer frames in their FIFO order.
 pub fn purge_unowned<const B:usize,const Q:usize,const L:usize>(
  &mut self,broker:&Broker<B,Q>,layout:&SurfaceLayout<L>)->usize{
  let mut surviving=[None;N];let mut count=0;
  for frame in self.entries[..self.len].iter().flatten(){
   let valid=frame.authority.is_some_and(|a|
    broker.binding(frame.owner,frame.window).is_some_and(|b|b.epoch==a.grant)&&
    layout.epoch(frame.owner,frame.window)==Some(a.surface))&&
    layout.geometry(frame.owner,frame.window).ok().is_some_and(|g|
     frame.width!=0&&frame.height!=0&&
     frame.x.checked_add(frame.width).is_some_and(|end|end<=g.width)&&
     frame.y.checked_add(frame.height).is_some_and(|end|end<=g.height));
   if valid{surviving[count]=Some(*frame);count+=1;}
  }
  let removed=self.len-count;
  self.entries=surviving;self.len=count;removed
 }
 /// Trusted, current-authorization-aware presentation. A short result
 /// buffer does not consume remaining valid work.
 pub fn present_fair_verified<const B:usize,const Q:usize,const L:usize>(
  &mut self,broker:&Broker<B,Q>,layout:&SurfaceLayout<L>,
  previous:Option<u32>,out:&mut[FrameDamage])
  ->Result<(u32,u64,usize),Error>{
  self.purge_unowned(broker,layout);
  self.present_fair(previous,out)
 }
 /// Atomically invalidate a window's draw work when its trusted owner
 /// withdraws the grant. A wrong owner must neither revoke the grant nor
 /// consume anyone's queued frames.
 pub fn revoke_window_with_damage<const B:usize,const Q:usize>(
  &mut self,broker:&mut Broker<B,Q>,owner:u32,window:u32)
  ->Result<usize,Error>{
  broker.revoke_window(owner,window)?;
  Ok(self.discard_window(owner,window))
 }
 /// A window ID reused by another process never inherits old owner frames.
 /// Validate the existing grant and new target before mutating either state.
 pub fn transfer_window_with_damage<const B:usize,const Q:usize>(
  &mut self,broker:&mut Broker<B,Q>,old_owner:u32,new_owner:u32,
  window:u32)->Result<usize,Error>{
  broker.reassign_window(old_owner,new_owner,window)?;
  Ok(self.discard_window(old_owner,window))
 }
 /// Transfer several app windows and drop every stale queued frame
 /// after the broker accepts the entire transaction. Invalid batches leave
 /// broker input and damage FIFO unchanged. Actual compositor surface
 /// invalidation and recreation remain the trusted caller's responsibility.
 pub fn transfer_batch_with_damage<const B:usize,const Q:usize>(
  &mut self,broker:&mut Broker<B,Q>,requests:&[(u32,u32,u32)])
  ->Result<usize,Error>{
  broker.reassign_batch(requests)?;
  let mut kept=[None;N];let mut n=0;
  for frame in self.entries[..self.len].iter().flatten(){
   if requests.iter().any(|&(_,_,window)|window==frame.window){continue;}
   kept[n]=Some(*frame);n+=1;
  }
  let removed=self.len-n;
  self.entries=kept;self.len=n;Ok(removed)
 }
 pub fn oldest_frame(&self)->Option<FrameDamage>{
  self.entries[..self.len].iter().flatten().min_by_key(|e|e.epoch).map(|e|e.frame)
 }
 pub fn discard_window(&mut self,owner:u32,window:u32)->usize{
  let mut kept=[None;N];let mut used=0;
  for item in self.entries[..self.len].iter().flatten(){
   if item.owner!=owner||item.window!=window{
    kept[used]=Some(*item);used+=1;
   }
  }
  let removed=self.len-used;self.entries=kept;self.len=used;removed
 }
}
#[cfg(test)]
mod r014_compositor_coalesce_tests{
 use super::*;use crate::agent_05_surface_rect::Rectangle;
 fn layout()->SurfaceLayout<3>{
  let mut l=SurfaceLayout::new();
  l.grant(3,10,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  l.grant(4,20,Rectangle{x:0,y:0,width:80,height:80}).unwrap();l
 }
 fn blank()->FrameDamage{FrameDamage{owner:0,window:0,epoch:0,
  x:0,y:0,width:0,height:0}}
 #[test]fn r057_atomic_multiwindow_transfer_discards_stale_frames(){
  let mut broker=Broker::<4,8>::new();
  broker.grant_batch(&[(3,10),(3,11),(4,20)]).unwrap();
  let mut layout=SurfaceLayout::<4>::new();
  for (owner,win) in [(3,10),(3,11),(4,20)]{
   layout.grant(owner,win,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  }
  let mut damage=DamageQueue::<6>::new();
  damage.submit_verified(&broker,&layout,3,&[(10,0,0,5,5),(11,0,0,5,5)]).unwrap();
  damage.submit_verified(&broker,&layout,4,&[(20,0,0,5,5)]).unwrap();
  let peer=broker.post_focus(4,20).unwrap();
  broker.post_focus(3,10).unwrap();
  assert_eq!(damage.transfer_batch_with_damage(&mut broker,&[(3,5,10),(3,5,11)]),
    Ok(2));
  assert_eq!(broker.owner(10),Some(5));
  assert_eq!(broker.owner(11),Some(5));
  assert_eq!(broker.peek(4),Ok(peer));
  assert_eq!(broker.pending_owner(5),Ok(0));
  assert_eq!(damage.pending(),1);
  assert_eq!(damage.oldest_frame().unwrap().window,20);
 }
 #[test]fn r057_invalid_batch_leaves_damage_and_owner_events_untouched(){
  let mut broker=Broker::<4,8>::new();
  broker.grant_batch(&[(3,10),(3,11),(4,20)]).unwrap();
  let mut layout=SurfaceLayout::<4>::new();
  for (owner,win) in [(3,10),(3,11),(4,20)]{
   layout.grant(owner,win,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  }
  let mut damage=DamageQueue::<6>::new();
  damage.submit_verified(&broker,&layout,3,&[(10,0,0,5,5),(11,0,0,5,5)]).unwrap();
  damage.submit_verified(&broker,&layout,4,&[(20,0,0,5,5)]).unwrap();
  let existing=broker.post_focus(3,10).unwrap();
  assert_eq!(damage.transfer_batch_with_damage(&mut broker,
   &[(3,5,10),(99,6,11)]),Err(Error::Unauthorized));
  assert_eq!(damage.pending(),3);
  assert_eq!(broker.peek(3),Ok(existing));
  assert_eq!(broker.owner(10),Some(3));
  assert_eq!(broker.owner(11),Some(3));
  assert_eq!(broker.owner(20),Some(4));
  assert_eq!(broker.post_focus(3,11).unwrap().sequence,existing.sequence+1);
 }
 #[test]fn m1_r052_compositor_drops_revoked_window_without_peer_damage(){
  let mut broker=Broker::<3,8>::new();
  broker.grant(3,10).unwrap();broker.grant(4,20).unwrap();
  let mut layout=layout();let mut q=DamageQueue::<4>::new();
  q.submit_verified(&broker,&layout,3,&[(10,1,1,4,4)]).unwrap();
  q.submit_verified(&broker,&layout,4,&[(20,2,2,3,3)]).unwrap();
  broker.revoke_window(3,10).unwrap();layout.revoke(3);
  assert_eq!(q.purge_unowned(&broker,&layout),1);
  assert_eq!(q.pending(),1);
  let mut out=[blank();1];
  assert_eq!(q.present_fair_verified(&broker,&layout,None,&mut out).unwrap().0,4);
  assert_eq!(out[0].owner,4);
 }
 #[test]fn m1_r052_compositor_short_buffer_keeps_valid_peer_frames(){
  let mut broker=Broker::<3,8>::new();
  broker.grant(3,10).unwrap();broker.grant(4,20).unwrap();
  let layout=layout();let mut q=DamageQueue::<4>::new();
  q.submit_verified(&broker,&layout,4,&[(20,1,1,2,2),(20,4,4,2,2)]).unwrap();
  let mut short=[blank();1];
  assert_eq!(q.present_fair_verified(&broker,&layout,None,&mut short),
   Err(Error::Full));
  assert_eq!(short,[blank()]);assert_eq!(q.pending(),2);
  let mut full=[blank();2];
  assert_eq!(q.present_fair_verified(&broker,&layout,None,&mut full).unwrap().2,2);
  assert_eq!(q.pending(),0);
 }
 #[test]fn m1_r053_window_close_discards_only_matching_damage_and_events(){
  let mut broker=Broker::<3,8>::new();
  broker.grant(3,10).unwrap();broker.grant(4,20).unwrap();
  let l=layout();let mut d=DamageQueue::<4>::new();
  d.submit(&l,3,&[(10,1,1,3,3)]).unwrap();
  d.submit(&l,4,&[(20,2,2,2,2)]).unwrap();
  broker.post_focus(3,10).unwrap();
  broker.post_focus(4,20).unwrap();
  assert_eq!(d.revoke_window_with_damage(&mut broker,4,10),Err(Error::Unauthorized));
  assert_eq!(d.pending(),2);
  assert_eq!(d.revoke_window_with_damage(&mut broker,3,10),Ok(1));
  assert_eq!(d.pending(),1);
  assert_eq!(broker.owner(10),None);
  assert_eq!(broker.pending_owner(4),Ok(1));
  assert_eq!(d.oldest_frame().unwrap().owner,4);
 }
 #[test]fn m1_r053_window_transfer_drops_old_frames_before_new_owner_can_draw(){
  let mut broker=Broker::<3,8>::new();
  broker.grant(3,10).unwrap();broker.grant(4,20).unwrap();
  let mut d=DamageQueue::<4>::new();let mut l=layout();
  d.submit(&l,3,&[(10,0,0,5,5)]).unwrap();
  d.submit(&l,4,&[(20,0,0,5,5)]).unwrap();
  assert_eq!(d.transfer_window_with_damage(&mut broker,99,4,10),
             Err(Error::Unauthorized));
  assert_eq!(d.pending(),2);
  assert_eq!(d.transfer_window_with_damage(&mut broker,3,4,10),Ok(1));
  assert_eq!(broker.owner(10),Some(4));assert_eq!(d.pending(),1);
  l.revoke(3);
  l.grant(4,10,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  d.submit(&l,4,&[(10,0,0,2,2)]).unwrap();
  let mut out=[blank();2];
  assert_eq!(d.drain(&mut out),Ok(2));
  assert!(out.iter().all(|f|f.owner==4));
 }
 #[test]fn merges_repeated_window_damage_and_preserves_peer_frames(){
  let l=layout();let mut q=DamageQueue::<4>::new();
  q.submit(&l,3,&[(10,1,2,3,4)]).unwrap();
  q.submit(&l,4,&[(20,1,1,2,2)]).unwrap();
  q.submit(&l,3,&[(10,10,11,4,5)]).unwrap();
  assert_eq!(q.combine_window_damage(3,10),Ok(1));
  assert_eq!(q.pending(),2);assert_eq!(q.oldest_frame().unwrap().owner,3);
  let mut out=[blank();2];assert_eq!(q.drain(&mut out),Ok(2));
  assert_eq!((out[0].x,out[0].y,out[0].width,out[0].height),(1,2,13,14));
  assert_eq!(out[1].owner,4);
 }
 #[test]fn wrong_owner_and_missing_window_have_no_side_effects(){
  let l=layout();let mut q=DamageQueue::<2>::new();
  q.submit(&l,3,&[(10,1,1,2,2)]).unwrap();
  assert_eq!(q.combine_window_damage(4,10),Ok(0));
  assert_eq!(q.discard_window(4,10),0);assert_eq!(q.pending(),1);
 }
 #[test]fn window_close_disposes_only_owner_local_damage(){
  let l=layout();let mut q=DamageQueue::<2>::new();
  q.submit(&l,3,&[(10,1,1,2,2)]).unwrap();
  q.submit(&l,4,&[(20,1,1,2,2)]).unwrap();
  assert_eq!(q.discard_window(3,10),1);assert_eq!(q.pending(),1);
  assert_eq!(q.oldest_frame().unwrap().owner,4);
 }
}

/// R016: non-consuming event view plus atomic acknowledgement for a ring-3
/// compositor. The caller's owner must come from the scheduler, not a packet.
impl<const W:usize,const Q:usize> Broker<W,Q>{
 pub fn preview_events(&self,owner:u32,out:&mut[Event])
  ->Result<usize,Error>{
  if !self.known(owner){return Err(Error::Unauthorized);}
  let count=self.events[..self.len].iter().flatten()
   .filter(|e|e.owner==owner).count();
  if count>out.len(){return Err(Error::Full);}
  let mut n=0;
  for item in self.events[..self.len].iter().flatten(){
   if item.owner==owner{out[n]=*item;n+=1;}
  }
  Ok(n)
 }
 pub fn acknowledge_events(&mut self,owner:u32,serials:&[u32])
  ->Result<usize,Error>{
  if !self.known(owner){return Err(Error::Unauthorized);}
  if serials.is_empty()||serials.len()>self.len{return Err(Error::Invalid);}
  for (i,&seq) in serials.iter().enumerate(){
   if seq==0||serials[..i].contains(&seq){return Err(Error::Invalid);}
   if !self.events[..self.len].iter().flatten()
       .any(|e|e.owner==owner&&e.sequence==seq){
    return Err(Error::Unauthorized);
   }
  }
  for &seq in serials{self.ack(owner,seq)?;}
  Ok(serials.len())
 }
 pub fn cancel_owned_events(&mut self,owner:u32)->Result<usize,Error>{
  if !self.known(owner){return Err(Error::Unauthorized);}
  let mut removed=0;let mut i=0;
  while i<self.len{
   if self.events[i].is_some_and(|e|e.owner==owner){
    self.remove(i);removed+=1;
   }else{i+=1;}
  }
  Ok(removed)
 }
}
#[cfg(test)]mod r016_gui_event_transactions_tests{
 use super::*;
 #[test]fn preview_and_ack_preserve_other_clients(){
  let mut b=Broker::<2,4>::new();b.grant(1,11).unwrap();b.grant(2,22).unwrap();
  let a=b.post_focus(1,11).unwrap();
  let peer=b.post_focus(2,22).unwrap();
  b.post_pointer_focus(1,11).unwrap();
  let mut events=[a;2];
  assert_eq!(b.preview_events(1,&mut events),Ok(2));
  assert_eq!(events[0].sequence,a.sequence);
  assert_eq!(b.acknowledge_events(1,&[a.sequence]),Ok(1));
  assert_eq!(b.peek(2),Ok(peer));
 }
 #[test]fn invalid_ack_batch_cannot_discard_valid_event(){
  let mut b=Broker::<2,3>::new();b.grant(1,11).unwrap();b.grant(2,22).unwrap();
  let first=b.post_focus(1,11).unwrap();
  let foreign=b.post_focus(2,22).unwrap();
  assert_eq!(b.acknowledge_events(1,&[first.sequence,foreign.sequence]),
    Err(Error::Unauthorized));
  assert_eq!(b.peek(1),Ok(first));
  assert_eq!(b.acknowledge_events(1,&[first.sequence,first.sequence]),
    Err(Error::Invalid));
 }
 #[test]fn cancellation_is_owner_scoped_and_short_preview_is_atomic(){
  let mut b=Broker::<2,3>::new();b.grant(1,11).unwrap();b.grant(2,22).unwrap();
  let first=b.post_focus(1,11).unwrap();
  b.post_pointer_focus(1,11).unwrap();let peer=b.post_focus(2,22).unwrap();
  let mut small=[peer;1];
  assert_eq!(b.preview_events(1,&mut small),Err(Error::Full));
  assert_eq!(small,[peer]);
  assert_eq!(b.cancel_owned_events(1),Ok(2));
  assert_eq!(b.peek(2),Ok(peer));
  assert_eq!(b.acknowledge_events(1,&[first.sequence]),Err(Error::Unauthorized));
 }
}



/// R017 developer UI input stream: acknowledge only a contiguous owner
/// prefix after the compositor has safely applied it.
impl<const W:usize,const Q:usize> Broker<W,Q>{
 pub fn preview_workstation_input(&self,owner:u32,out:&mut[Event])
  ->Result<usize,Error>{self.preview_events(owner,out)}
 pub fn acknowledge_through(&mut self,owner:u32,serial:u32)
  ->Result<usize,Error>{
  if serial==0||!self.known(owner){return Err(Error::Unauthorized);}
  let mut numbers=[0u32;Q];let mut count=0;
  for event in self.events[..self.len].iter().flatten(){
   if event.owner==owner&&event.sequence<=serial{
    numbers[count]=event.sequence;count+=1;
   }
  }
  if count==0{return Err(Error::Empty);}
  if numbers[count-1]!=serial{return Err(Error::Unauthorized);}
  self.acknowledge_events(owner,&numbers[..count])
 }
 pub fn pending_input_for(&self,owner:u32)->Result<usize,Error>{
  self.pending_owner(owner)
 }
}
#[cfg(test)]mod r017_workstation_input_stream_tests{
 use super::*;
 #[test]fn ack_owner_prefix_does_not_erase_peer_events(){
  let mut b=Broker::<2,5>::new();b.grant(1,10).unwrap();b.grant(2,20).unwrap();
  let first=b.post_focus(1,10).unwrap();let peer=b.post_focus(2,20).unwrap();
  let last=b.post_pointer_focus(1,10).unwrap();
  assert_eq!(b.acknowledge_through(1,last.sequence),Ok(2));
  assert_eq!(b.peek(2),Ok(peer));assert_eq!(b.pending_input_for(1),Ok(0));
  assert!(first.sequence<last.sequence);
 }
 #[test]fn invented_serial_is_denied_without_mutation(){
  let mut b=Broker::<1,2>::new();b.grant(1,10).unwrap();
  let ev=b.post_focus(1,10).unwrap();
  assert_eq!(b.acknowledge_through(1,ev.sequence+1),
    Err(Error::Unauthorized));assert_eq!(b.peek(1),Ok(ev));
 }
 #[test]fn preview_never_consumes_and_short_output_is_denied(){
  let mut b=Broker::<1,2>::new();b.grant(1,10).unwrap();
  b.post_focus(1,10).unwrap();b.post_pointer_focus(1,10).unwrap();
  let mut out=[Event{owner:9,window:9,kind:9,sequence:9};1];
  assert_eq!(b.preview_workstation_input(1,&mut out),Err(Error::Full));
  assert_eq!(out[0].owner,9);assert_eq!(b.pending_input_for(1),Ok(2));
 }
}

#[cfg(test)]
mod m1_input_grant_lifecycle_tests {
 use super::*;
 #[test]
 fn withdrawn_grant_cannot_resurrect_focus_or_capture() {
  for reassign in [false,true] {
   for pointer in [false,true] {
    let mut b=Broker::<2,8>::new();b.grant(1,10).unwrap();b.grant(2,20).unwrap();
    let mut input=DesktopInput::new();input.focus(&mut b,10).unwrap();
    input.capture_pointer(&b,1,10).unwrap();
    let peer=b.post_focus(2,20).unwrap();
    if reassign {b.reassign_window(1,3,10).unwrap();b.reassign_window(3,1,10).unwrap();}
    else {b.revoke_window(1,10).unwrap();b.grant(1,10).unwrap();}
    assert_eq!(input.capture_pointer(&b,2,20),Err(Error::Unauthorized));
    assert_eq!(input.capture_pointer(&b,1,10),Err(Error::Unauthorized));
    let result=if pointer {input.click(&mut b,1)} else {input.keyboard(&mut b,30,true)};
    assert_eq!(result,Err(Error::Unauthorized));
    assert_eq!(input.focused(),None);assert_eq!(input.captured(),None);
    assert_eq!(b.peek(1),Err(Error::Empty));assert_eq!(b.peek(2),Ok(peer));
    input.focus(&mut b,10).unwrap();input.capture_pointer(&b,1,10).unwrap();
    assert_eq!(input.keyboard(&mut b,30,true).unwrap().window,10);
    assert_eq!(input.click(&mut b,1).unwrap().owner,1);
    assert_eq!(b.peek(2),Ok(peer));
   }
  }
 }
 #[test]
 fn full_queue_preserves_focus_and_capture() {
  let mut b=Broker::<2,1>::new();b.grant(1,10).unwrap();b.grant(2,20).unwrap();
  let mut input=DesktopInput::new();let target=input.focus(&mut b,10).unwrap();
  input.capture_pointer(&b,1,10).unwrap();
  assert_eq!(input.focus(&mut b,20),Err(Error::Full));
  assert_eq!(input.focused(),Some(target));assert_eq!(input.captured(),Some(target));
  let event=b.peek(1).unwrap();b.ack(1,event.sequence).unwrap();
  assert_eq!(input.click(&mut b,1).unwrap().owner,1);
 }
 #[test]
 fn grant_exhaustion_is_atomic_and_batches_track_epochs() {
  let mut b=Broker::<4,4>::new();b.grant(1,10).unwrap();
  let event=b.post_focus(1,10).unwrap();
  b.grant_epoch=u64::MAX-1;
  assert_eq!(b.grant_batch(&[(2,20),(3,30)]),Err(Error::Exhausted));
  assert_eq!(b.grant_epoch,u64::MAX-1);assert!(!b.owns(2,20));
  assert_eq!(b.grant_batch(&[(2,20)]),Ok(1));assert_eq!(b.grant_epoch,u64::MAX);
  assert_eq!(b.grant(3,30),Err(Error::Exhausted));
  assert_eq!(b.reassign_window(1,3,10),Err(Error::Exhausted));
  assert!(b.owns(1,10));assert_eq!(b.peek(1),Ok(event));
  b.revoke_window(1,10).unwrap();assert_eq!(b.grant(1,10),Err(Error::Exhausted));
 }
}

#[cfg(test)]
mod m1_damage_authority_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn blank()->FrameDamage{FrameDamage{owner:0,window:0,epoch:0,
  x:0,y:0,width:0,height:0}}
 fn setup()->(Broker<3,4>,SurfaceLayout<3>,DamageQueue<8>){
  let mut b=Broker::new();let mut l=SurfaceLayout::new();
  for (owner,window) in [(1,10),(2,20)]{
   b.grant(owner,window).unwrap();
   l.grant(owner,window,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  }
  (b,l,DamageQueue::new())
 }
 #[test]fn m1_damage_recycled_grant_cannot_resurrect_frame(){
  for transfer in [false,true]{
   let (mut b,l,mut q)=setup();
   q.submit_verified(&b,&l,1,&[(10,1,1,4,4)]).unwrap();
   let peer=q.submit_verified(&b,&l,2,&[(20,2,2,3,3)]).unwrap();
   if transfer{b.reassign_window(1,3,10).unwrap();b.reassign_window(3,1,10).unwrap();}
   else{b.revoke_window(1,10).unwrap();b.grant(1,10).unwrap();}
   let fresh=q.submit_verified(&b,&l,1,&[(10,3,3,2,2)]).unwrap();
   assert_eq!(q.combine_window_damage(1,10),Err(Error::Unauthorized));
   assert_eq!(q.purge_unowned(&b,&l),1);
   let mut out=[blank();1];
   assert_eq!(q.present_fair_verified(&b,&l,None,&mut out),Ok((2,peer,1)));
   assert_eq!(q.present_fair_verified(&b,&l,Some(2),&mut out),Ok((1,fresh,1)));
   assert_eq!(out[0].x,3);assert_eq!(q.pending(),0);
  }
 }
 #[test]fn m1_damage_recycled_surface_and_unbound_frames_fail_closed(){
  let (b,mut l,mut q)=setup();
  q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]).unwrap();
  q.submit(&l,2,&[(20,0,0,1,1)]).unwrap();
  l.revoke(1);
  l.grant(1,10,Rectangle{x:0,y:0,width:80,height:80}).unwrap();
  assert_eq!(q.purge_unowned(&b,&l),2);
  q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]).unwrap();
  let mut out=[blank();1];
  assert!(q.present_fair_verified(&b,&l,None,&mut out).is_ok());
 }
 #[test]fn m1_damage_submission_failure_preserves_queue_and_epoch(){
  let (b,l,mut q)=setup();
  let first=q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]).unwrap();
  assert_eq!(q.submit_verified(&b,&l,1,&[(10,0,0,1,1),(20,0,0,1,1)]),
   Err(Error::Unauthorized));
  assert_eq!(q.submit_verified(&b,&l,1,&[(10,0,0,1,1),(10,80,0,1,1)]),
   Err(Error::Invalid));
  assert_eq!(q.pending(),1);
  assert_eq!(q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]),Ok(first+1));
  let mut short=[blank();1];
  // Two separate frames; coalescing is allowed only within the same grants.
  assert_eq!(q.combine_window_damage(1,10),Ok(1));
  assert!(q.present_fair_verified(&b,&l,None,&mut short).is_ok());
 }
 #[test]fn m1_damage_moved_surface_rejects_pre_move_work(){
  let (b,mut l,mut q)=setup();
  q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]).unwrap();
  l.move_owned(1,10,1,1).unwrap();l.move_owned(1,10,-1,-1).unwrap();
  assert_eq!(q.purge_unowned(&b,&l),1);
  q.submit_verified(&b,&l,1,&[(10,0,0,1,1)]).unwrap();
  let geometry=l.geometry(1,10).unwrap();l.next_epoch=u64::MAX;
  assert_eq!(l.move_owned(1,10,1,1),Err(Error::Exhausted));
  assert_eq!(l.geometry(1,10),Ok(geometry));
  assert_eq!(q.purge_unowned(&b,&l),0);
 }
 #[test]fn m1_damage_surface_epoch_exhaustion_does_not_publish(){
  let (_,mut l,_)=setup();l.next_epoch=u64::MAX;
  assert_eq!(l.grant(3,30,Rectangle{x:0,y:0,width:4,height:4}),Err(Error::Exhausted));
  assert_eq!(l.geometry(3,30),Err(Error::Unauthorized));
  assert!(l.geometry(1,10).is_ok());
 }
}


/// R060: compositor-owned horizontal tiling for developer workspaces.
/// Entire target geometry, ownership and epochs are validated by the
/// existing atomic geometry transaction before any window is moved.
/// This is metadata only; the userspace compositor must render the tiles.
impl<const N:usize> SurfaceLayout<N>{
 pub fn tile_horizontal(&mut self,
   windows:&[(u32,u32)],
   area:crate::agent_05_surface_rect::Rectangle)->Result<usize,Error>{
  use crate::agent_05_surface_rect::{Rectangle,clip_owned};
  if windows.is_empty()||windows.len()>N||
     windows.len()>area.width as usize{return Err(Error::Invalid);}
  clip_owned(area,area,1,1).map_err(|_|Error::Invalid)?;
  let count=windows.len() as u32; // bounded by validated screen width
  let base=area.width/count;
  let surplus=area.width%count;
  let empty=Rectangle{x:0,y:0,width:1,height:1};
  let mut request=[(0u32,0u32,empty);N];
  let mut x=area.x;
  for (i,&(owner,window)) in windows.iter().enumerate(){
   let width=base+u32::from((i as u32)<surplus);
   request[i]=(owner,window,Rectangle{
     x,y:area.y,width,height:area.height
   });
   x=x.checked_add(width).ok_or(Error::Invalid)?;
  }
  self.set_geometry_batch(&request[..windows.len()])
 }
}
#[cfg(test)]
mod r060_tiling_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn rect(x:u32,y:u32,w:u32,h:u32)->Rectangle{
  Rectangle{x,y,width:w,height:h}
 }
 #[test]fn tile_three_owned_windows_with_exact_remaining_pixel_distribution(){
  let mut layout=SurfaceLayout::<4>::new();
  for (owner,window) in [(1,10),(2,20),(3,30)]{
   layout.grant(owner,window,rect(4,4,20,20)).unwrap();
  }
  let previous=layout.epoch(1,10).unwrap();
  assert_eq!(layout.tile_horizontal(&[(1,10),(2,20),(3,30)],
      rect(20,30,101,70)),Ok(3));
  assert_eq!(layout.geometry(1,10),Ok(rect(20,30,34,70)));
  assert_eq!(layout.geometry(2,20),Ok(rect(54,30,34,70)));
  assert_eq!(layout.geometry(3,30),Ok(rect(88,30,33,70)));
  assert!(layout.epoch(1,10).unwrap()>previous);
 }
 #[test]fn invalid_late_owner_duplicate_and_offscreen_area_roll_back(){
  let mut layout=SurfaceLayout::<2>::new();
  let original=rect(5,5,30,30);
  layout.grant(1,10,original).unwrap();
  layout.grant(2,20,original).unwrap();
  let epoch=layout.epoch(1,10);
  assert_eq!(layout.tile_horizontal(&[(1,10),(9,20)],
      rect(0,0,200,100)),Err(Error::Unauthorized));
  assert_eq!(layout.tile_horizontal(&[(1,10),(1,10)],
      rect(0,0,200,100)),Err(Error::Duplicate));
  assert_eq!(layout.tile_horizontal(&[(1,10),(2,20)],
      rect(1270,0,100,100)),Err(Error::Invalid));
  assert_eq!(layout.geometry(1,10),Ok(original));
  assert_eq!(layout.geometry(2,20),Ok(original));
  assert_eq!(layout.epoch(1,10),epoch);
 }
}


/// R061: 2-D developer workstation grid planner. Unlike merely moving
/// windows, this packs each authenticated surface into a disjoint tile in
/// the target rectangle and atomically publishes geometry/epoch changes.
/// The actual user-space compositor must still consume the new layout.
impl<const N:usize> SurfaceLayout<N>{
 pub fn tile_grid(&mut self,windows:&[(u32,u32)],columns:usize,
  area:crate::agent_05_surface_rect::Rectangle)->Result<usize,Error>{
  use crate::agent_05_surface_rect::{Rectangle,clip_owned};
  if windows.is_empty()||windows.len()>N||columns==0||
     columns>windows.len()||columns>area.width as usize{
    return Err(Error::Invalid);
  }
  clip_owned(area,area,1,1).map_err(|_|Error::Invalid)?;
  let rows=windows.len()/columns+usize::from(windows.len()%columns!=0);
  if rows>area.height as usize{return Err(Error::Invalid);}
  let cols=columns as u32;
  let rows=rows as u32;
  let width=area.width/cols;
  let width_extra=area.width%cols;
  let height=area.height/rows;
  let height_extra=area.height%rows;
  let empty=Rectangle{x:0,y:0,width:1,height:1};
  let mut requests=[(0u32,0u32,empty);N];
  for (i,&(owner,window)) in windows.iter().enumerate(){
   let c=(i%columns) as u32;
   let r=(i/columns) as u32;
   let left=area.x.checked_add(c*width)
     .and_then(|x|x.checked_add(c.min(width_extra)))
     .ok_or(Error::Invalid)?;
   let top=area.y.checked_add(r*height)
     .and_then(|y|y.checked_add(r.min(height_extra)))
     .ok_or(Error::Invalid)?;
   requests[i]=(owner,window,Rectangle{
    x:left,y:top,width:width+u32::from(c<width_extra),
    height:height+u32::from(r<height_extra)
   });
  }
  self.set_geometry_batch(&requests[..windows.len()])
 }
}
#[cfg(test)]
mod r061_grid_layout_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn r(x:u32,y:u32,w:u32,h:u32)->Rectangle{
  Rectangle{x,y,width:w,height:h}
 }
 #[test]fn odd_dimension_three_window_grid_covers_every_pixel_without_overlap(){
  let mut layout=SurfaceLayout::<4>::new();
  for (owner,window) in [(1,10),(2,20),(3,30)]{
   layout.grant(owner,window,r(5,5,30,30)).unwrap();
  }
  assert_eq!(layout.tile_grid(&[(1,10),(2,20),(3,30)],2,
    r(20,30,101,81)),Ok(3));
  assert_eq!(layout.geometry(1,10),Ok(r(20,30,51,41)));
  assert_eq!(layout.geometry(2,20),Ok(r(71,30,50,41)));
  assert_eq!(layout.geometry(3,30),Ok(r(20,71,51,40)));
  let epoch=layout.epoch(1,10);
  assert_eq!(layout.tile_grid(&[(1,10),(2,20),(3,30)],2,
    r(20,30,101,81)),Ok(3));
  assert_eq!(layout.epoch(1,10),epoch);
 }
 #[test]fn invalid_late_owner_duplicate_and_bad_grid_do_not_move_any_window(){
  let mut layout=SurfaceLayout::<3>::new();
  let old=r(5,5,20,20);
  layout.grant(1,10,old).unwrap();layout.grant(2,20,old).unwrap();
  let before=layout.epoch(1,10);
  assert_eq!(layout.tile_grid(&[(1,10),(9,20)],2,r(0,0,100,100)),
    Err(Error::Unauthorized));
  assert_eq!(layout.tile_grid(&[(1,10),(1,10)],2,r(0,0,100,100)),
    Err(Error::Duplicate));
  assert_eq!(layout.tile_grid(&[(1,10),(2,20)],0,r(0,0,100,100)),
    Err(Error::Invalid));
  assert_eq!(layout.tile_grid(&[(1,10),(2,20)],2,r(0,0,1,100)),
    Err(Error::Invalid));
  assert_eq!(layout.tile_grid(&[(1,10),(2,20)],2,r(1279,0,100,100)),
    Err(Error::Invalid));
  assert_eq!(layout.geometry(1,10),Ok(old));
  assert_eq!(layout.geometry(2,20),Ok(old));
  assert_eq!(layout.epoch(1,10),before);
 }
}


/// R062: stack authenticated developer windows into a visually accessible
/// cascade. Each later surface is shifted, so no window silently overlays
/// its predecessor completely. All geometry, owner/epoch and screen-bound
/// checks are performed by the existing shadow-transaction layout setter.
impl<const N:usize> SurfaceLayout<N>{
 pub fn cascade_windows(&mut self,windows:&[(u32,u32)],
    first:crate::agent_05_surface_rect::Rectangle,
    step_x:u32,step_y:u32)->Result<usize,Error>{
  use crate::agent_05_surface_rect::Rectangle;
  if windows.is_empty()||windows.len()>N||step_x==0||step_y==0{
   return Err(Error::Invalid);
  }
  let mut updates=[(0u32,0u32,Rectangle{x:0,y:0,width:1,height:1});N];
  for (i,&(owner,window)) in windows.iter().enumerate(){
   let index=u32::try_from(i).map_err(|_|Error::Invalid)?;
   let dx=step_x.checked_mul(index).ok_or(Error::Invalid)?;
   let dy=step_y.checked_mul(index).ok_or(Error::Invalid)?;
   let x=first.x.checked_add(dx).ok_or(Error::Invalid)?;
   let y=first.y.checked_add(dy).ok_or(Error::Invalid)?;
   updates[i]=(owner,window,Rectangle{
    x,y,width:first.width,height:first.height
   });
  }
  self.set_geometry_batch(&updates[..windows.len()])
 }
}
#[cfg(test)]
mod r062_cascade_layout_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn rect(x:u32,y:u32,w:u32,h:u32)->Rectangle{
  Rectangle{x,y,width:w,height:h}
 }
 #[test]fn three_owned_windows_cascade_with_distinct_redraw_epochs(){
  let mut layout=SurfaceLayout::<3>::new();
  let initial=rect(10,10,100,90);
  for (o,w) in [(1,10),(2,20),(3,30)]{
   layout.grant(o,w,initial).unwrap();
  }
  let before=layout.epoch(2,20);
  assert_eq!(layout.cascade_windows(&[(1,10),(2,20),(3,30)],
    rect(20,30,160,100),18,24),Ok(3));
  assert_eq!(layout.geometry(1,10),Ok(rect(20,30,160,100)));
  assert_eq!(layout.geometry(2,20),Ok(rect(38,54,160,100)));
  assert_eq!(layout.geometry(3,30),Ok(rect(56,78,160,100)));
  assert!(layout.epoch(2,20)>before);
 }
 #[test]fn late_foreign_owner_and_screen_overflow_preserve_every_window(){
  let mut layout=SurfaceLayout::<2>::new();
  let original=rect(5,5,30,30);
  layout.grant(1,10,original).unwrap();
  layout.grant(2,20,original).unwrap();
  let old=layout.epoch(1,10);
  assert_eq!(layout.cascade_windows(&[(1,10),(9,20)],
   rect(10,10,50,50),10,10),Err(Error::Unauthorized));
  assert_eq!(layout.cascade_windows(&[(1,10),(2,20)],
   rect(1200,700,100,100),10,10),Err(Error::Invalid));
  assert_eq!(layout.cascade_windows(&[(1,10),(1,10)],
   rect(10,10,50,50),10,10),Err(Error::Duplicate));
  assert_eq!(layout.cascade_windows(&[(1,10),(2,20)],
   rect(10,10,50,50),0,10),Err(Error::Invalid));
  assert_eq!(layout.epoch(1,10),old);
  assert_eq!(layout.geometry(1,10),Ok(original));
  assert_eq!(layout.geometry(2,20),Ok(original));
 }
}


/// R063: add a visible gutter/inset around each tile without applying
/// an intermediate layout. The temporary grid validates owner, window,
/// screen and geometry; inset preflight then publishes each final rectangle
/// with one atomic epoch update. This avoids flicker and partial redraws.
impl<const N:usize> SurfaceLayout<N>{
 pub fn tile_grid_inset(&mut self,windows:&[(u32,u32)],
    columns:usize,area:crate::agent_05_surface_rect::Rectangle,
    inset:u32)->Result<usize,Error>{
  use crate::agent_05_surface_rect::Rectangle;
  if inset==0{return self.tile_grid(windows,columns,area);}
  let pad=inset.checked_mul(2).ok_or(Error::Invalid)?;
  let mut staged=Self{surfaces:self.surfaces,epochs:self.epochs,
    next_epoch:self.next_epoch};
  staged.tile_grid(windows,columns,area)?;
  let zero=Rectangle{x:0,y:0,width:1,height:1};
  let mut requests=[(0u32,0u32,zero);N];
  for (i,&(owner,window)) in windows.iter().enumerate(){
   let r=staged.geometry(owner,window)?;
   if r.width<=pad||r.height<=pad{return Err(Error::Invalid);}
   requests[i]=(owner,window,Rectangle{
    x:r.x.checked_add(inset).ok_or(Error::Invalid)?,
    y:r.y.checked_add(inset).ok_or(Error::Invalid)?,
    width:r.width-pad,height:r.height-pad
   });
  }
  self.set_geometry_batch(&requests[..windows.len()])
 }
}
#[cfg(test)]
mod r063_padded_grid_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn r(x:u32,y:u32,w:u32,h:u32)->Rectangle{
  Rectangle{x,y,width:w,height:h}
 }
 #[test]fn padded_three_window_grid_is_exact_and_single_epoch_commit(){
  let mut layout=SurfaceLayout::<3>::new();
  for (owner,window) in [(1,10),(2,20),(3,30)]{
   layout.grant(owner,window,r(0,0,20,20)).unwrap();
  }
  // Initial grants already consumed 3 epochs. One atomic layout
  // transaction assigns a DISTINCT next epoch to EACH changed window.
  let before=layout.next_epoch;
  assert_eq!(layout.tile_grid_inset(
    &[(1,10),(2,20),(3,30)],2,r(20,30,101,81),3),Ok(3));
  assert_eq!(layout.geometry(1,10),Ok(r(23,33,45,35)));
  assert_eq!(layout.geometry(2,20),Ok(r(74,33,44,35)));
  assert_eq!(layout.geometry(3,30),Ok(r(23,74,45,34)));
  assert_eq!(layout.epoch(1,10),Some(before+1));
  assert_eq!(layout.epoch(2,20),Some(before+2));
  assert_eq!(layout.epoch(3,30),Some(before+3));
  assert_eq!(layout.next_epoch,before+3);
  // Repeating an unchanged geometry layout must not consume epochs.
  assert_eq!(layout.tile_grid_inset(
    &[(1,10),(2,20),(3,30)],2,r(20,30,101,81),3),Ok(3));
  assert_eq!(layout.next_epoch,before+3);
 }
 #[test]fn excessive_inset_and_late_unauthorized_owner_are_atomic(){
  let mut layout=SurfaceLayout::<2>::new();
  let old=r(5,5,20,20);
  layout.grant(1,10,old).unwrap();
  layout.grant(2,20,old).unwrap();
  let epoch=layout.epoch(1,10);
  assert_eq!(layout.tile_grid_inset(
    &[(1,10),(2,20)],2,r(0,0,20,20),5),Err(Error::Invalid));
  assert_eq!(layout.tile_grid_inset(
    &[(1,10),(9,20)],2,r(0,0,100,100),1),Err(Error::Unauthorized));
  assert_eq!(layout.tile_grid_inset(
    &[(1,10),(1,10)],2,r(0,0,100,100),1),Err(Error::Duplicate));
  assert_eq!(layout.geometry(1,10),Ok(old));
  assert_eq!(layout.geometry(2,20),Ok(old));
  assert_eq!(layout.epoch(1,10),epoch);
 }
}


/// R064: vertical window tiling for terminal/editor/log panels.
/// Every pane gets a nonzero height; odd pixels are distributed from the
/// top. Ownership and final geometry are published in ONE validated
/// transaction with no intermediate compositor-visible positions.
impl<const N:usize> SurfaceLayout<N>{
 pub fn tile_vertical(&mut self,windows:&[(u32,u32)],
    area:crate::agent_05_surface_rect::Rectangle)->Result<usize,Error>{
  use crate::agent_05_surface_rect::{Rectangle,clip_owned};
  if windows.is_empty()||windows.len()>N||
     windows.len()>area.height as usize{return Err(Error::Invalid);}
  clip_owned(area,area,1,1).map_err(|_|Error::Invalid)?;
  let count=u32::try_from(windows.len()).map_err(|_|Error::Invalid)?;
  let base=area.height/count;
  let extra=area.height%count;
  let empty=Rectangle{x:0,y:0,width:1,height:1};
  let mut requests=[(0u32,0u32,empty);N];
  let mut y=area.y;
  for (i,&(owner,window)) in windows.iter().enumerate(){
   let height=base+u32::from((i as u32)<extra);
   requests[i]=(owner,window,Rectangle{
    x:area.x,y,width:area.width,height
   });
   y=y.checked_add(height).ok_or(Error::Invalid)?;
  }
  self.set_geometry_batch(&requests[..windows.len()])
 }
}
#[cfg(test)]
mod r064_vertical_tiles_tests{
 use super::*;
 use crate::agent_05_surface_rect::Rectangle;
 fn r(x:u32,y:u32,w:u32,h:u32)->Rectangle{
  Rectangle{x,y,width:w,height:h}
 }
 #[test]fn three_owned_windows_use_full_vertical_area_and_noop_keeps_epochs(){
  let mut layout=SurfaceLayout::<3>::new();
  let old=r(4,5,30,30);
  for (owner,window) in [(1,10),(2,20),(3,30)]{
   layout.grant(owner,window,old).unwrap();
  }
  let before=layout.next_epoch;
  assert_eq!(layout.tile_vertical(&[(1,10),(2,20),(3,30)],
   r(20,30,80,61)),Ok(3));
  assert_eq!(layout.geometry(1,10),Ok(r(20,30,80,21)));
  assert_eq!(layout.geometry(2,20),Ok(r(20,51,80,20)));
  assert_eq!(layout.geometry(3,30),Ok(r(20,71,80,20)));
  assert_eq!(layout.next_epoch,before+3);
  assert_eq!(layout.tile_vertical(&[(1,10),(2,20),(3,30)],
   r(20,30,80,61)),Ok(3));
  assert_eq!(layout.next_epoch,before+3);
 }
 #[test]fn unauthorized_window_duplicate_and_offscreen_area_roll_back(){
  let mut layout=SurfaceLayout::<2>::new();
  let old=r(2,2,40,40);
  layout.grant(1,10,old).unwrap();
  layout.grant(2,20,old).unwrap();
  let before=layout.next_epoch;
  assert_eq!(layout.tile_vertical(&[(1,10),(9,20)],
   r(10,10,60,60)),Err(Error::Unauthorized));
  assert_eq!(layout.tile_vertical(&[(1,10),(1,10)],
   r(10,10,60,60)),Err(Error::Duplicate));
  assert_eq!(layout.tile_vertical(&[(1,10),(2,20)],
   r(0,0,30,1)),Err(Error::Invalid));
  assert_eq!(layout.tile_vertical(&[(1,10),(2,20)],
   r(u32::MAX,0,60,100)),Err(Error::Invalid));
  assert_eq!(layout.next_epoch,before);
  assert_eq!(layout.geometry(1,10),Ok(old));
  assert_eq!(layout.geometry(2,20),Ok(old));
 }
}
