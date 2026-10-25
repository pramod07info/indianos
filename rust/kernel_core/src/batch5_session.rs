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
 pub fn next_service(&self)->Result<Option<Service>,InitError>{self.services.next()}
 pub fn service_ready(&mut self,id:u16)->Result<(),InitError>{self.services.ready(id)}
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
  self.keyboard.set_focus(&self.windows,Focus{
   pid:identity.pid,generation:identity.generation,window
  }).map_err(SessionError::Input)
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
 /// Called by the trusted process-reap path, before PID reuse.
 pub fn retire(&mut self,identity:Identity)->Result<(),SessionError>{
  if !self.owns(identity){return Err(SessionError::Unauthorized);}
  self.launches.retire(identity.pid,identity.generation);
  self.events.retire(identity.pid,identity.generation);
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