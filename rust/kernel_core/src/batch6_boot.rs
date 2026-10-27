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
/// Bound graphical service crash restarts for each boot session.
pub const MAX_DESKTOP_RESTARTS:u8=3;
/// Exposes only verified runtime ownership, never claims physical framebuffer
/// or driver startup without the corresponding real QEMU boot evidence.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BootHealth{
 pub phase:Phase,
 pub pid1:Option<Identity>,
 pub desktop:Option<Identity>,
 pub remaining_desktop_restarts:u8,
 pub ready_for_desktop_input:bool,
}

/// Kernel-service blockers are classified from actual scheduler ownership.
/// This is a diagnosis for the boot supervisor, not hardware state probing.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BootBlocker{
 Cold,InitMissing,DesktopMissing,DesktopRestartExhausted,Stopped,Ready,
}
pub struct Bootstrap<const P:usize,const E:usize,const W:usize,const K:usize>{
 pub session:Session<P,E,W,K>,phase:Phase,init:Option<Identity>,
 desktop:Option<Identity>,desktop_restarts:u8,
}
impl<const P:usize,const E:usize,const W:usize,const K:usize>
 Bootstrap<P,E,W,K>{
 pub const fn new()->Self{Self{session:Session::new(),
  phase:Phase::Cold,init:None,desktop:None,desktop_restarts:0}}
 pub fn phase(&self)->Phase{self.phase}
 pub fn desktop_restart_count(&self)->u8{self.desktop_restarts}
 /// Fail-closed service health for native startup and recovery supervision.
 /// A mere DesktopReady phase is insufficient if the owning PCB has gone.
 pub fn health(&self)->BootHealth{
  let pid1=self.init.filter(|x|self.session.owns(*x));
  let desktop=self.desktop.filter(|x|self.session.owns(*x));
  BootHealth{
   phase:self.phase,
   pid1,desktop,
   remaining_desktop_restarts:MAX_DESKTOP_RESTARTS
      .saturating_sub(self.desktop_restarts),
   ready_for_desktop_input:self.phase==Phase::DesktopReady
     &&pid1.is_some()&&desktop.is_some(),
  }
 }

 /// Disambiguate incomplete startup, restart budget exhaustion and the
 /// irrevocable loss of PID1 before admitting trusted desktop input.
 pub fn blocker(&self)->BootBlocker{
  let health=self.health();
  match health.phase{
   Phase::Cold=>BootBlocker::Cold,
   Phase::Stopped=>BootBlocker::Stopped,
   Phase::InitReady=>if health.pid1.is_some(){
     BootBlocker::DesktopMissing
    }else{BootBlocker::InitMissing},
   Phase::DesktopReady=>if health.pid1.is_none(){
      BootBlocker::InitMissing
     }else if health.desktop.is_none(){
      BootBlocker::DesktopMissing
     }else{BootBlocker::Ready},
   Phase::Degraded=>if health.pid1.is_none(){
      BootBlocker::InitMissing
     }else if health.remaining_desktop_restarts==0{
      BootBlocker::DesktopRestartExhausted
     }else{BootBlocker::DesktopMissing},
  }
 }
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
 /// Restore only the desktop service after a crash, while the original
 /// authenticated init process is still alive. Does not resurrect PID1,
 /// bypass image verification, or claim the hardware boot path is wired.
 /// All prior windows, focus and IPC were revoked by process_exited.
 pub fn recover_desktop(&mut self,id:Identity,launch:Launch,
  settings:Settings)->Result<(),Error>{
  if self.phase!=Phase::Degraded||self.desktop.is_some()||
     self.desktop_restarts>=MAX_DESKTOP_RESTARTS||
     self.init.is_none_or(|init|!self.session.owns(init)||init.pid==id.pid)||
     self.session.next_service().map_err(Error::Service)?.is_some(){
   return Err(Error::State);
  }
  // Prove that the trusted loader/scheduler identity matches the proposed
  // image *before* modifying the running desktop session.
  if id.pid==0||id.generation==0||
     id.pid!=launch.pid||id.generation!=launch.generation{
   return Err(Error::State);
  }
  self.session.stage(launch,settings).map_err(Error::Session)?;
  self.desktop=Some(id);
  self.desktop_restarts+=1;
  self.phase=Phase::DesktopReady;
  Ok(())
 }
 pub fn process_exited(&mut self,id:Identity)->Result<(),Error>{
  // PID1 is the lifetime root of the graphical session. Once it dies,
  // all desktop ownership (windows, keyboard focus, IPC and settings)
  // must be invalidated before the caller can recycle a process ID.
  self.session.retire(id).map_err(Error::Session)?;
  if self.init==Some(id){
   if let Some(desktop)=self.desktop.take(){
    if self.session.owns(desktop){
     self.session.retire(desktop).map_err(Error::Session)?;
    }
   }
   self.init=None;
   self.phase=Phase::Stopped;
  }else if self.desktop==Some(id){
   self.desktop=None;
   self.phase=Phase::Degraded;
  }
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
 #[test]fn init_exit_revokes_desktop_focus_windows_and_launches(){
  use crate::batch2_windows::{Window,Rect};
  use crate::batch5_session::SessionError;
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  b.session.create_window(id(2),Window{pid:2,generation:1,id:8,
    rect:Rect{x:0,y:0,w:100,h:80}}).unwrap();
  b.session.focus(id(2),8).unwrap();
  b.session.physical_key(30,true).unwrap();
  b.process_exited(id(1)).unwrap();
  assert_eq!(b.phase(),Phase::Stopped);
  assert_eq!(b.desktop_identity(),None);
  assert!(!b.session.owns(id(1)));
  assert!(!b.session.owns(id(2)));
  assert!(b.session.windows.get(8).is_none());
  assert_eq!(b.session.receive_key(id(2)),Err(SessionError::Unauthorized));
  assert!(b.session.physical_key(30,true).is_err());
  assert!(b.session.consume_launch(id(2)).is_err());
 }
 #[test]fn unsigned_boot_image_cannot_activate_service(){
  let mut b=Bootstrap::<2,2,2,2>::new();b.configure(1,2).unwrap();
  let mut a=launch(id(1));a.image.verified=false;
  assert_eq!(b.start_init(id(1),a,settings(),1),
   Err(Error::Session(SessionError::Launch(
    crate::batch2_launcher::LaunchError::Unverified))));
  assert_eq!(b.phase(),Phase::Cold);
 }

 #[test]fn supervised_desktop_crash_restart_rejects_stale_windows(){
  use crate::batch2_windows::{Rect,Window};
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  b.session.create_window(id(2),Window{pid:2,generation:1,id:40,
      rect:Rect{x:0,y:0,w:40,h:40}}).unwrap();
  b.session.focus(id(2),40).unwrap();
  b.process_exited(id(2)).unwrap();
  let resumed=Identity{pid:2,generation:2};
  b.recover_desktop(resumed,launch(resumed),settings()).unwrap();
  assert_eq!(b.phase(),Phase::DesktopReady);
  assert_eq!(b.desktop_identity(),Some(resumed));
  assert!(!b.session.windows.owns(2,1,40));
  assert_eq!(b.session.receive_key(id(2)),Err(SessionError::Unauthorized));
  assert!(b.session.owns(id(1)));
 }
 #[test]fn desktop_restart_requires_live_init_and_verified_image(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  b.process_exited(id(2)).unwrap();
  let newer=Identity{pid:2,generation:3};
  let mut bad=launch(newer);bad.image.verified=false;
  assert_eq!(b.recover_desktop(newer,bad,settings()),
            Err(Error::Session(SessionError::Launch(
              crate::batch2_launcher::LaunchError::Unverified))));
  assert_eq!(b.phase(),Phase::Degraded);
  assert!(!b.session.owns(newer));
  b.process_exited(id(1)).unwrap();
  assert_eq!(b.recover_desktop(newer,launch(newer),settings()),
            Err(Error::State));
 }
 #[test]fn desktop_restart_rejects_identity_spoofing_without_staging(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  b.process_exited(id(2)).unwrap();
  let newer=Identity{pid:2,generation:2};
  assert_eq!(b.recover_desktop(newer,launch(id(2)),settings()),
            Err(Error::State));
  assert_eq!(b.recover_desktop(id(1),launch(id(1)),settings()),
            Err(Error::State));
  assert!(!b.session.owns(newer));
  assert_eq!(b.phase(),Phase::Degraded);
  b.recover_desktop(newer,launch(newer),settings()).unwrap();
  assert_eq!(b.recover_desktop(newer,launch(newer),settings()),
            Err(Error::State));
 }


 #[test]fn failing_desktop_restarts_are_bounded_per_boot_session(){
  let mut b=Bootstrap::<3,3,3,3>::new();b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  for generation in 2..=4u64{
   let old=Identity{pid:2,generation:generation-1};
   b.process_exited(old).unwrap();
   let now=Identity{pid:2,generation};
   assert_eq!(b.recover_desktop(now,launch(now),settings()),Ok(()));
   assert_eq!(b.desktop_restart_count(),(generation-1) as u8);
  }
  b.process_exited(Identity{pid:2,generation:4}).unwrap();
  let fifth=Identity{pid:2,generation:5};
  assert_eq!(b.recover_desktop(fifth,launch(fifth),settings()),Err(Error::State));
  assert_eq!(b.phase(),Phase::Degraded);
  assert!(!b.session.owns(fifth));
  assert!(b.session.owns(id(1)));
 }
 #[test]fn failed_restart_does_not_consume_budget(){
  let mut b=Bootstrap::<3,3,3,3>::new();b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  b.process_exited(id(2)).unwrap();
  let new=Identity{pid:2,generation:2};
  let mut bad=launch(new);bad.image.verified=false;
  assert!(b.recover_desktop(new,bad,settings()).is_err());
  assert_eq!(b.desktop_restart_count(),0);
  b.recover_desktop(new,launch(new),settings()).unwrap();
  assert_eq!(b.desktop_restart_count(),1);
 }


 #[test]fn health_only_reports_real_live_init_and_desktop(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  assert!(!b.health().ready_for_desktop_input);
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  assert_eq!(b.health().pid1,Some(id(1)));
  assert!(!b.health().ready_for_desktop_input);
  b.start_desktop(id(2),launch(id(2)),settings(),20).unwrap();
  assert!(b.health().ready_for_desktop_input);
  b.process_exited(id(2)).unwrap();
  assert!(!b.health().ready_for_desktop_input);
  assert_eq!(b.health().desktop,None);
  let newer=Identity{pid:2,generation:2};
  b.recover_desktop(newer,launch(newer),settings()).unwrap();
  assert!(b.health().ready_for_desktop_input);
  assert_eq!(b.health().remaining_desktop_restarts,MAX_DESKTOP_RESTARTS-1);
  b.process_exited(id(1)).unwrap();
  assert_eq!(b.health().phase,Phase::Stopped);
  assert_eq!(b.health().desktop,None);
  assert!(!b.health().ready_for_desktop_input);
 }
 #[test]fn unsigned_boot_never_reports_ready_for_user_input(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  b.configure(10,20).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),10).unwrap();
  let mut invalid=launch(id(2));invalid.image.verified=false;
  assert!(b.start_desktop(id(2),invalid,settings(),20).is_err());
  assert!(!b.health().ready_for_desktop_input);
 }


 #[test]fn boot_blockers_track_authenticated_service_lifecycles(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  assert_eq!(b.blocker(),BootBlocker::Cold);
  b.configure(11,22).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),11).unwrap();
  assert_eq!(b.blocker(),BootBlocker::DesktopMissing);
  b.start_desktop(id(2),launch(id(2)),settings(),22).unwrap();
  assert_eq!(b.blocker(),BootBlocker::Ready);
  b.process_exited(id(2)).unwrap();
  assert_eq!(b.blocker(),BootBlocker::DesktopMissing);
  b.process_exited(id(1)).unwrap();
  assert_eq!(b.blocker(),BootBlocker::Stopped);
 }
 #[test]fn repeated_desktop_restarts_eventually_report_exhaustion(){
  let mut b=Bootstrap::<3,3,3,3>::new();b.configure(11,22).unwrap();
  b.start_init(id(1),launch(id(1)),settings(),11).unwrap();
  b.start_desktop(id(2),launch(id(2)),settings(),22).unwrap();
  for generation in 2..=4{
   let current=Identity{pid:2,generation:generation-1};
   b.process_exited(current).unwrap();
   assert_eq!(b.blocker(),BootBlocker::DesktopMissing);
   let next=Identity{pid:2,generation};
   b.recover_desktop(next,launch(next),settings()).unwrap();
  }
  b.process_exited(Identity{pid:2,generation:4}).unwrap();
  assert_eq!(b.blocker(),BootBlocker::DesktopRestartExhausted);
 }

}