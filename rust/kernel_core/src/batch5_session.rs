//! Batch 5 tasks 2,3,4,7,9: coherent kernel-trusted guest session policy.
//! Scheduler, ELF loader, real compositor, keyboard IRQ and syscall ingress
//! MUST provide authenticated PID/generation; none run here automatically.
use crate::batch2_init::{InitPlan,InitError,Service};
use crate::batch2_launcher::{LaunchQueue,Launch,LaunchError};
use crate::batch2_events::{Broker,BrokerError,Event};
use crate::batch2_windows::{Windows,Window,WindowError};
use crate::batch2_keyboard::{Router,Focus,InputError,KeyEvent};
use crate::batch2_settings::{Store,Settings,SettingsError};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SessionError{Invalid,Duplicate,Full,Unauthorized,Launch(LaunchError),
 Window(WindowError),Event(BrokerError),Input(InputError),Settings(SettingsError)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Identity{pub pid:u32,pub generation:u64}
pub struct Session<const P:usize,const E:usize,const W:usize,const K:usize>{
 owners:[Option<Identity>;P],
 pub services:InitPlan<P>,
 pub launches:LaunchQueue<P>,
 pub events:Broker<E>,
 pub windows:Windows<W>,
 pub keyboard:Router<K>,
 pub settings:Store<P>,
}
impl<const P:usize,const E:usize,const W:usize,const K:usize>
 Session<P,E,W,K>{
 pub const fn new()->Self{Self{
  owners:[None;P],services:InitPlan::new(),
  launches:LaunchQueue::new(),events:Broker::new(),
  windows:Windows::new(),keyboard:Router::new(),settings:Store::new()
 }}
 pub fn register_service(&mut self,service:Service)->Result<(),InitError>{
  self.services.register(service)
 }
 /// Fail closed on any dangling/cyclic service dependency, even when
 /// another root could otherwise launch successfully.
 pub fn next_service(&self)->Result<Option<Service>,InitError>{
  self.services.next_validated()
 }
 pub fn service_ready(&mut self,id:u16)->Result<(),InitError>{
  self.services.validate_graph()?;
  self.services.ready(id)
 }
 pub fn owns(&self,identity:Identity)->bool{
  identity.pid!=0 && identity.generation!=0 &&
  self.owners.iter().flatten().any(|&s|s==identity)
 }
 /// Only trusted launch pipeline should call this AFTER image verification.
 pub fn stage(&mut self,launch:Launch,settings:Settings)->Result<(),SessionError>{
  let identity=Identity{pid:launch.pid,generation:launch.generation};
  if identity.pid==0||identity.generation==0{return Err(SessionError::Invalid);}
  if self.owners.iter().flatten().any(|s|s.pid==identity.pid){
   return Err(SessionError::Duplicate);
  }
  let free=self.owners.iter().position(Option::is_none).ok_or(SessionError::Full)?;
  self.launches.stage(launch).map_err(SessionError::Launch)?;
  // Only one owner per PID. If settings staging fails, roll back intent.
  if let Err(error)=self.settings.register(identity.pid,identity.generation,settings){
   self.launches.retire(identity.pid,identity.generation);
   return Err(SessionError::Settings(error));
  }
  self.owners[free]=Some(identity);Ok(())
 }
 pub fn consume_launch(&mut self,identity:Identity)->Result<Launch,SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  self.launches.take(identity.pid,identity.generation).map_err(SessionError::Launch)
 }
 pub fn create_window(&mut self,identity:Identity,window:Window)
     ->Result<(),SessionError>{
  if !self.owns(identity)||window.pid!=identity.pid||
   window.generation!=identity.generation{return Err(SessionError::Unauthorized);}
  self.windows.create(window).map_err(SessionError::Window)
 }
 pub fn focus(&mut self,identity:Identity,window:u32)->Result<(),SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  let requested=Focus{pid:identity.pid,generation:identity.generation,window};
  // A new focus must never inherit keystrokes queued for the old owner.
  // Preserve pending keystrokes when the existing exact focus is unchanged.
  if self.keyboard.focused()==Some(requested){
   return self.keyboard.set_focus(&self.windows,requested)
    .map_err(SessionError::Input);
  }
  self.keyboard.switch_focus_drop_pending(&self.windows,requested)
   .map(|_|()).map_err(SessionError::Input)
 }
 /// Only a trusted physical IRQ dispatch source can call this.
 pub fn physical_key(&mut self,scan:u16,pressed:bool)->Result<(),SessionError>{
  self.keyboard.physical_key(&self.windows,scan,pressed)
   .map_err(SessionError::Input)
 }
 pub fn receive_key(&mut self,identity:Identity)->Result<KeyEvent,SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  self.keyboard.receive(identity.pid,identity.generation)
   .map_err(SessionError::Input)
 }
 pub fn send(&mut self,authenticated_sender:Identity,recipient:Identity,
  kind:u16,body:&[u8])->Result<(),SessionError>{
  if !self.owns(authenticated_sender)||!self.owns(recipient){
   return Err(SessionError::Unauthorized);
  }
  self.events.send(authenticated_sender.pid,recipient.pid,recipient.generation,
   kind,body).map_err(SessionError::Event)
 }
 pub fn receive(&mut self,identity:Identity)->Result<Event,SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  self.events.receive(identity.pid,identity.generation)
   .map_err(SessionError::Event)
 }
 /// Close an exact process-owned window and revoke its queued input before
 /// another window can reuse the public ID. The kernel's compositor/PS2
 /// dispatch must invoke this after authenticating the caller.
 pub fn close_window(&mut self,owner:Identity,id:u32)
  ->Result<(),SessionError>{
  if !self.owns(owner)||!self.windows.owns(owner.pid,owner.generation,id){
   return Err(SessionError::Unauthorized);
  }
  self.windows.close_exact(owner.pid,owner.generation,id)
   .map_err(SessionError::Window)?;
  self.keyboard.cancel_window(owner.pid,owner.generation,id)
   .map_err(SessionError::Input)?;
  self.keyboard.clear_focus_exact(Focus{
   pid:owner.pid,generation:owner.generation,window:id
  });
  Ok(())
 }
 /// Authenticate every recipient before publishing a multi-recipient group.
 /// Queue capacity, payload validity and the entire group are committed
 /// atomically by the underlying broker. A bad destination leaves it empty.
 pub fn send_batch(&mut self,sender:Identity,
  messages:&[(Identity,u16,&[u8])])->Result<usize,SessionError>{
  if !self.owns(sender){return Err(SessionError::Unauthorized);}
  if messages.is_empty()||messages.len()>E{
   return Err(SessionError::Event(BrokerError::Invalid));
  }
  let mut batch:[(u32,u64,u16,&[u8]);E]=[(0,0,0,&[]);E];
  for (i,&(recipient,kind,data)) in messages.iter().enumerate(){
   if !self.owns(recipient){return Err(SessionError::Unauthorized);}
   batch[i]=(recipient.pid,recipient.generation,kind,data);
  }
  self.events.send_batch(sender.pid,&batch[..messages.len()])
   .map_err(SessionError::Event)
 }
 /// Exact scheduler incarnation is required to drain the whole mailbox.
 /// A short output slice must leave messages owned by this recipient intact.
 pub fn receive_batch(&mut self,owner:Identity,out:&mut[Event])
  ->Result<usize,SessionError>{
  if !self.owns(owner){return Err(SessionError::Unauthorized);}
  self.events.receive_batch(owner.pid,owner.generation,out)
   .map_err(SessionError::Event)
 }
 /// Called by the trusted process-reap path, before PID reuse.
 pub fn retire(&mut self,identity:Identity)->Result<(),SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  self.launches.retire(identity.pid,identity.generation);
  self.events.retire(identity.pid,identity.generation);
  // Outgoing messages must not outlive the sending PID incarnation and
  // be delivered to a recipient after sender task exit/PID recycling.
  self.events.drop_sender(identity.pid).map_err(SessionError::Event)?;
  self.windows.retire(identity.pid,identity.generation);
  self.keyboard.retire(identity.pid,identity.generation);
  self.settings.retire(identity.pid,identity.generation);
  for slot in &mut self.owners{
   if slot.is_some_and(|x|x==identity){*slot=None;}
  }
  Ok(())
 }
}
#[cfg(test)] mod tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_windows::Rect;
 use crate::batch2_settings::Theme;
 fn launch(id:Identity)->Launch{Launch{pid:id.pid,generation:id.generation,
   image:Image{image_id:10,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:4,key_repeat:12}}
 #[test]fn process_exit_revokes_window_focus_ipc_and_settings(){
  let mut r=Session::<2,3,2,2>::new();
  let a=Identity{pid:2,generation:4};let b=Identity{pid:3,generation:1};
  r.stage(launch(a),settings()).unwrap();r.stage(launch(b),settings()).unwrap();
  assert_eq!(r.consume_launch(a),Ok(launch(a)));
  r.create_window(a,Window{pid:2,generation:4,id:1,
   rect:Rect{x:2,y:3,w:100,h:100}}).unwrap();
  r.focus(a,1).unwrap();r.physical_key(30,true).unwrap();
  r.send(b,a,8,b"hello").unwrap();
  assert_eq!(r.receive_key(a).unwrap().scan_code,30);
  r.retire(a).unwrap();
  assert!(!r.windows.owns(2,4,1));
  assert_eq!(r.receive(a),Err(SessionError::Unauthorized));
  assert_eq!(r.send(b,a,8,b"old"),Err(SessionError::Unauthorized));
  assert_eq!(r.physical_key(30,true),Err(SessionError::Input(InputError::NoFocus)));
  let newer=Identity{pid:2,generation:5};
  r.stage(launch(newer),settings()).unwrap();
  assert_eq!(r.receive(newer),Err(SessionError::Event(BrokerError::Empty)));
 }
 #[test]fn unsigned_images_and_identity_spoofing_fail_closed(){
  let mut r=Session::<1,1,1,1>::new();
  let id=Identity{pid:1,generation:2};let mut x=launch(id);
  x.image.verified=false;
  assert_eq!(r.stage(x,settings()),Err(SessionError::Launch(LaunchError::Unverified)));
  assert!(!r.owns(id));
  r.stage(launch(id),settings()).unwrap();
  assert_eq!(r.consume_launch(Identity{pid:1,generation:3}),Err(SessionError::Unauthorized));
 }
}
#[cfg(test)]
mod r085_m1_lifecycle_integration_tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 use crate::batch2_windows::Rect;
 fn launch(i:Identity)->Launch{Launch{pid:i.pid,generation:i.generation,
   image:Image{image_id:10,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:4,key_repeat:12}}
 fn add<const P:usize,const E:usize,const W:usize,const K:usize>(
  s:&mut Session<P,E,W,K>,i:Identity){
  s.stage(launch(i),settings()).unwrap();
 }
 const EMPTY:Event=Event{sender:0,recipient:0,generation:0,kind:0,
   len:0,payload:[0;crate::batch2_events::PAYLOAD_BYTES]};
 #[test]
 fn dead_sender_messages_are_reaped_before_pid_reuse(){
  let mut s=Session::<3,5,2,4>::new();
  let a=Identity{pid:3,generation:1};
  let b=Identity{pid:5,generation:1};
  add(&mut s,a);add(&mut s,b);
  s.send(a,b,10,b"should-not-arrive").unwrap();
  s.send(b,a,11,b"also-discarded").unwrap();
  assert_eq!(s.events.pending_total(),2);
  s.retire(a).unwrap();
  assert_eq!(s.events.pending_total(),0);
  let recycled=Identity{pid:3,generation:2};
  add(&mut s,recycled);
  assert_eq!(s.receive(b),Err(SessionError::Event(BrokerError::Empty)));
  assert_eq!(s.receive(recycled),Err(SessionError::Event(BrokerError::Empty)));
  assert_eq!(s.send(a,b,10,b"stale"),Err(SessionError::Unauthorized));
  s.send(recycled,b,10,b"fresh").unwrap();
  assert_eq!(s.receive(b).unwrap().payload[0],b'f');
 }
 #[test]
 fn focus_switch_discards_old_keys_without_touching_new_owner(){
  let mut s=Session::<3,4,2,6>::new();
  let a=Identity{pid:2,generation:8};let b=Identity{pid:3,generation:9};
  add(&mut s,a);add(&mut s,b);
  s.create_window(a,Window{pid:2,generation:8,id:1,
   rect:Rect{x:0,y:0,w:60,h:60}}).unwrap();
  s.create_window(b,Window{pid:3,generation:9,id:2,
   rect:Rect{x:10,y:10,w:60,h:60}}).unwrap();
  s.focus(a,1).unwrap();s.physical_key(30,true).unwrap();
  s.focus(a,1).unwrap(); // same lease preserves pending input
  assert_eq!(s.keyboard.pending_for(2,8),1);
  s.focus(b,2).unwrap(); // cross-owner focus transition revokes pending
  assert_eq!(s.receive_key(a),Err(SessionError::Input(InputError::Empty)));
  s.physical_key(31,true).unwrap();
  assert_eq!(s.receive_key(b).unwrap().scan_code,31);
  assert_eq!(s.close_window(a,2),Err(SessionError::Unauthorized));
  assert_eq!(s.close_window(b,2),Ok(()));
  assert_eq!(s.keyboard.focused(),None);
  assert_eq!(s.physical_key(10,true),Err(SessionError::Input(InputError::NoFocus)));
 }
 #[test]
 fn valid_only_group_publish_and_short_output_never_partially_commit(){
  let mut s=Session::<3,4,1,2>::new();
  let a=Identity{pid:2,generation:10};
  let b=Identity{pid:3,generation:1};
  add(&mut s,a);add(&mut s,b);
  let stale=Identity{pid:3,generation:2};
  assert_eq!(s.send_batch(a,&[(b,1,b"one"),(stale,2,b"bad")]),
    Err(SessionError::Unauthorized));
  assert_eq!(s.events.pending_total(),0);
  assert_eq!(s.send_batch(a,&[(b,1,b"one"),(b,2,b"two")]),Ok(2));
  let mut too_small=[EMPTY;1];
  assert_eq!(s.receive_batch(b,&mut too_small),
    Err(SessionError::Event(BrokerError::Full)));
  assert_eq!(too_small,[EMPTY]);
  assert_eq!(s.events.pending_total(),2);
  let mut out=[EMPTY;2];
  assert_eq!(s.receive_batch(b,&mut out),Ok(2));
  assert_eq!(out[0].kind,1);assert_eq!(out[1].kind,2);
  assert_eq!(s.events.pending_total(),0);
 }
 #[test]
 fn invalid_dependency_prevents_any_root_launch_selection(){
  let mut s=Session::<3,1,1,1>::new();
  s.register_service(Service{id:1,dependency:None,essential:true}).unwrap();
  s.register_service(Service{id:2,dependency:Some(99),essential:true}).unwrap();
  assert_eq!(s.next_service(),Err(InitError::MissingDependency));
 }
}
