//! R040 tasks 2/4/7/9: authenticated init-to-service activation transaction.
//! The trusted ELF loader/scheduler supplies process identities and images;
//! this no_std controller does not start a CPU or claim normal boot is wired.
use crate::batch2_init::{InitError,Service};
use crate::batch2_launcher::Launch;
use crate::batch2_settings::Settings;
use crate::batch5_session::{Identity,Session,SessionError};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{State,Service(InitError),Session(SessionError)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Phase{Cold,InitReady,DesktopReady,Degraded,Stopped}
pub struct Bootstrap<const P:usize,const E:usize,const W:usize,const K:usize>{
 pub session:Session<P,E,W,K>,phase:Phase,init:Option<Identity>,
 desktop:Option<Identity>,
}
impl<const P:usize,const E:usize,const W:usize,const K:usize>
 Bootstrap<P,E,W,K>{
 pub const fn new()->Self{Self{session:Session::new(),
  phase:Phase::Cold,init:None,desktop:None}}
 pub fn phase(&self)->Phase{self.phase}
 pub fn configure(&mut self,init_id:u16,desktop_id:u16)->Result<(),Error>{
  if self.phase!=Phase::Cold||init_id==0||desktop_id==init_id{
   return Err(Error::State);
  }
  self.session.register_service(Service{id:init_id,dependency:None,essential:true})
   .map_err(Error::Service)?;
  self.session.register_service(Service{id:desktop_id,
   dependency:Some(init_id),essential:true}).map_err(Error::Service)
 }
 pub fn start_init(&mut self,id:Identity,launch:Launch,
  settings:Settings,service_id:u16)->Result<(),Error>{
  if self.phase!=Phase::Cold||self.init.is_some()||
   self.session.next_service().map_err(Error::Service)?
     !=Some(Service{id:service_id,dependency:None,essential:true}){
   return Err(Error::State);
  }
  self.session.stage(launch,settings).map_err(Error::Session)?;
  if launch.pid!=id.pid||launch.generation!=id.generation{
   self.session.retire(Identity{pid:launch.pid,generation:launch.generation})
    .map_err(Error::Session)?;
   return Err(Error::State);
  }
  self.session.service_ready(service_id).map_err(Error::Service)?;
  self.init=Some(id);self.phase=Phase::InitReady;Ok(())
 }
 pub fn start_desktop(&mut self,id:Identity,launch:Launch,
  settings:Settings,service_id:u16)->Result<(),Error>{
  if self.phase!=Phase::InitReady||self.desktop.is_some()||
   self.session.next_service().map_err(Error::Service)?
      .is_none_or(|s|s.id!=service_id){
   return Err(Error::State);
  }
  self.session.stage(launch,settings).map_err(Error::Session)?;
  if launch.pid!=id.pid||launch.generation!=id.generation{
   self.session.retire(Identity{pid:launch.pid,generation:launch.generation})
    .map_err(Error::Session)?;
   return Err(Error::State);
  }
  self.session.service_ready(service_id).map_err(Error::Service)?;
  self.desktop=Some(id);self.phase=Phase::DesktopReady;Ok(())
 }
 pub fn process_exited(&mut self,id:Identity)->Result<(),Error>{
  self.session.retire(id).map_err(Error::Session)?;
  if self.desktop==Some(id){self.desktop=None;self.phase=Phase::Degraded;}
  if self.init==Some(id){self.init=None;self.phase=Phase::Stopped;}
  Ok(())
 }
 pub fn desktop_identity(&self)->Option<Identity>{
  if self.phase==Phase::DesktopReady{self.desktop}else{None}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(pid:u32)->Identity{Identity{pid,generation:1}}
 fn launch(a:Identity)->Launch{Launch{pid:a.pid,generation:a.generation,
  image:Image{image_id:a.pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 #[test]fn verified_init_must_precede_desktop(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  assert_eq!(b.start_desktop(id(2),launch(id(2)),settings(),20),Err(Error::State));
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  assert_eq!(b.phase(),Phase::DesktopReady);
  b.process_exited(id(2)).unwrap();
  assert_eq!(b.phase(),Phase::Degraded);
  assert_eq!(b.desktop_identity(),None);
 }
 #[test]fn unsigned_boot_image_cannot_activate_service(){
  let mut b=Bootstrap::<2,2,2,2>::new();b.configure(1,2).unwrap();
  let mut a=launch(id(1));a.image.verified=false;
  assert_eq!(b.start_init(id(1),a,settings(),1),
   Err(Error::Session(SessionError::Launch(
    crate::batch2_launcher::LaunchError::Unverified))));
  assert_eq!(b.phase(),Phase::Cold);
 }
}