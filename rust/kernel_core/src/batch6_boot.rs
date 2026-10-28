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

/// R006: one transaction for a minimal graphical service dependency chain.
/// The trusted kernel must still instantiate CPUs, load ELF images and wire
/// real interrupts; this controller only manages authenticated services.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct GraphicalPlan{
 pub init_service:u16,pub desktop_service:u16,
 pub init:Identity,pub desktop:Identity,
 pub init_image:Launch,pub desktop_image:Launch,
 pub settings:Settings,
}
impl<const P:usize,const E:usize,const W:usize,const K:usize>
 Bootstrap<P,E,W,K>{
 /// Preflight every static boot invariant BEFORE publishing even PID1.
 /// Any unexpected service/staging failure restores an entirely cold plan.
 pub fn start_graphical_plan(&mut self,plan:GraphicalPlan)->Result<BootHealth,Error>{
  if self.phase!=Phase::Cold||self.init.is_some()||self.desktop.is_some()||
   P<2||plan.init_service==0||plan.desktop_service==0||
   plan.init_service==plan.desktop_service||
   plan.init.pid==0||plan.desktop.pid==0||
   plan.init.pid==plan.desktop.pid||
   plan.init.generation==0||plan.desktop.generation==0||
   plan.init_image.pid!=plan.init.pid||
   plan.init_image.generation!=plan.init.generation||
   plan.desktop_image.pid!=plan.desktop.pid||
   plan.desktop_image.generation!=plan.desktop.generation||
   !plan.init_image.image.verified||!plan.desktop_image.image.verified{
   return Err(Error::State);
  }
  let step=(||{
   self.configure(plan.init_service,plan.desktop_service)?;
   self.start_init(plan.init,plan.init_image,plan.settings,plan.init_service)?;
   self.start_desktop(plan.desktop,plan.desktop_image,plan.settings,
     plan.desktop_service)?;
   Ok::<(),Error>(())
  })();
  if let Err(reason)=step{
   *self=Self::new();
   return Err(reason);
  }
  if !self.health().ready_for_desktop_input{
   *self=Self::new();return Err(Error::State);
  }
  Ok(self.health())
 }
 /// Caller-authenticated desktop input gate. A stale process generation,
 /// orphaned compositor, or failed init never receives new UI events.
 pub fn admit_desktop_input(&self,caller:Identity)->Result<(),Error>{
  if caller.pid==0||caller.generation==0||
   self.blocker()!=BootBlocker::Ready||
   self.health().desktop!=Some(caller){
    return Err(Error::State);
  }
  Ok(())
 }
 /// Orderly shutdown of desktop then PID1, revoking service ownership.
 /// Unlike a raw phase transition this also retires windows and input.
 pub fn shutdown_graphical(&mut self,init:Identity)->Result<(),Error>{
  if self.init!=Some(init)||!self.session.owns(init){
   return Err(Error::State);
  }
  if let Some(desktop)=self.desktop{
   self.process_exited(desktop)?;
  }
  self.process_exited(init)
 }
}
#[cfg(test)]
mod r006_boot_transaction_tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(pid:u32)->Identity{Identity{pid,generation:4}}
 fn launch(i:Identity)->Launch{Launch{pid:i.pid,generation:i.generation,
   image:Image{image_id:i.pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn plan()->GraphicalPlan{
  GraphicalPlan{init_service:11,desktop_service:22,init:id(1),desktop:id(2),
   init_image:launch(id(1)),desktop_image:launch(id(2)),
   settings:Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 }
 #[test]fn complete_boot_plan_allows_only_owning_desktop(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  let status=b.start_graphical_plan(plan()).unwrap();
  assert_eq!(status.phase,Phase::DesktopReady);
  assert!(status.ready_for_desktop_input);
  assert_eq!(b.admit_desktop_input(id(2)),Ok(()));
  assert_eq!(b.admit_desktop_input(Identity{generation:5,..id(2)}),Err(Error::State));
  assert_eq!(b.shutdown_graphical(id(1)),Ok(()));
  assert_eq!(b.phase(),Phase::Stopped);
  assert_eq!(b.admit_desktop_input(id(2)),Err(Error::State));
 }
 #[test]fn failed_desktop_stage_rolls_entire_plan_back_to_cold(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  let mut p=plan();p.desktop_image.image.entry=0;
  assert!(b.start_graphical_plan(p).is_err());
  assert_eq!(b.phase(),Phase::Cold);
  assert!(!b.session.owns(id(1)));
  assert!(!b.session.owns(id(2)));
  assert!(b.start_graphical_plan(plan()).is_ok());
 }
 #[test]fn preflight_rejects_wrong_ownership_without_partial_stage(){
  let mut b=Bootstrap::<3,3,3,3>::new();
  let mut p=plan();p.desktop.generation=77;
  assert_eq!(b.start_graphical_plan(p),Err(Error::State));
  assert_eq!(b.phase(),Phase::Cold);
  let mut short=Bootstrap::<1,3,3,3>::new();
  assert_eq!(short.start_graphical_plan(plan()),Err(Error::State));
 }
}

 
/// R009: one supervisor transaction for a crashed desktop process. This
/// does not synthesize process identities: the caller must supply identities
/// authenticated by the kernel scheduler and a verified loader image.
impl<const P:usize,const E:usize,const W:usize,const K:usize>
 Bootstrap<P,E,W,K>{
 pub fn restart_failed_desktop(&mut self,dead:Identity,replacement:Identity,
   launch:Launch,settings:Settings)->Result<BootHealth,Error>{
  if self.phase!=Phase::DesktopReady||self.desktop!=Some(dead)||
     !self.session.owns(dead)||self.init.is_none_or(|i|!self.session.owns(i))||
     self.desktop_restarts>=MAX_DESKTOP_RESTARTS||
     replacement.pid==0||replacement.generation==0||
     replacement==dead||replacement.pid==self.init.unwrap().pid||
     replacement.pid!=launch.pid||replacement.generation!=launch.generation||
     self.session.owns(replacement){return Err(Error::State);}
  // All preflight checks run before tearing down the old desktop.
  self.process_exited(dead)?;
  self.recover_desktop(replacement,launch,settings)?;
  Ok(self.health())
 }
}
#[cfg(test)]
mod r009_desktop_restart_tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
 fn image(x:Identity)->Launch{Launch{pid:x.pid,generation:x.generation,
   image:Image{image_id:x.pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 fn ready()->Bootstrap<3,3,3,3>{
  let mut b=Bootstrap::new();let init=id(1,3);let desktop=id(2,3);
  b.configure(11,22).unwrap();
  b.start_init(init,image(init),settings(),11).unwrap();
  b.start_desktop(desktop,image(desktop),settings(),22).unwrap();b
 }
 #[test]fn crashed_desktop_restarts_without_losing_pid1(){
  let mut b=ready();let new=id(2,4);
  let status=b.restart_failed_desktop(id(2,3),new,image(new),settings()).unwrap();
  assert_eq!(status.phase,Phase::DesktopReady);
  assert_eq!(status.pid1,Some(id(1,3)));
  assert_eq!(status.desktop,Some(new));
  assert!(!b.session.owns(id(2,3)));
  assert_eq!(b.desktop_restart_count(),1);
 }
 #[test]fn forged_replacement_never_tears_down_healthy_desktop(){
  let mut b=ready();let fresh=id(2,4);
  assert_eq!(b.restart_failed_desktop(id(2,3),fresh,image(id(2,5)),settings()),Err(Error::State));
  assert_eq!(b.health().desktop,Some(id(2,3)));
  assert_eq!(b.desktop_restart_count(),0);
 }
 #[test]fn crash_restart_budget_exhaustion_fails_closed(){
  let mut b=ready();
  for gen in 4..=6{
   let newer=id(2,gen);
   b.restart_failed_desktop(id(2,gen-1),newer,image(newer),settings()).unwrap();
  }
  let next=id(2,7);
  assert_eq!(b.restart_failed_desktop(id(2,6),next,image(next),settings()),Err(Error::State));
  assert_eq!(b.health().desktop,Some(id(2,6)));
 }
}

 
/// R010 service restart throttle. The native scheduler must provide a
/// monotonic tick clock and verified identities; this is not a boot driver.
pub struct RestartGate{last_success:Option<u64>,minimum_ticks:u64}
impl RestartGate{
 pub const fn new(minimum_ticks:u64)->Self{
  Self{last_success:None,minimum_ticks}
 }
 pub fn last_success(&self)->Option<u64>{self.last_success}
 pub fn attempt<const P:usize,const E:usize,const W:usize,const K:usize>(
  &mut self,boot:&mut Bootstrap<P,E,W,K>,now:u64,
  dead:Identity,replacement:Identity,launch:Launch,settings:Settings)
  ->Result<BootHealth,Error>{
  if self.last_success.is_some_and(|then|
      now.checked_sub(then).is_none_or(|elapsed|elapsed<self.minimum_ticks)){
    return Err(Error::State);
  }
  // No clock or retry bookkeeping is committed until the owning scheduler
  // has retired the old process and fully staged a verified replacement.
  let health=boot.restart_failed_desktop(dead,replacement,launch,settings)?;
  self.last_success=Some(now);Ok(health)
 }
 /// A new authenticated user session resets only the throttle, not the
 /// owning bootstrap's separate global crash-restart budget.
 pub fn new_session(&mut self){self.last_success=None;}
}
#[cfg(test)]
mod r010_boot_throttle_tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(g:u64)->Identity{Identity{pid:2,generation:g}}
 fn launch(x:Identity)->Launch{Launch{pid:x.pid,generation:x.generation,
  image:Image{image_id:x.pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 fn running()->Bootstrap<3,3,3,3>{
  let mut boot=Bootstrap::<3,3,3,3>::new();
  let init=Identity{pid:1,generation:2};
  boot.configure(11,22).unwrap();
  boot.start_init(init,launch(init),settings(),11).unwrap();
  boot.start_desktop(id(2),launch(id(2)),settings(),22).unwrap();boot
 }
 #[test]fn restart_throttle_rejects_early_retry_before_modifying_boot(){
  let mut boot=running();let mut throttle=RestartGate::new(10);
  assert_eq!(throttle.attempt(&mut boot,20,id(2),id(3),launch(id(3)),settings())
    .unwrap().desktop,Some(id(3)));
  assert_eq!(throttle.attempt(&mut boot,25,id(3),id(4),launch(id(4)),settings()),
    Err(Error::State));
  assert_eq!(boot.health().desktop,Some(id(3)));
  assert_eq!(throttle.last_success(),Some(20));
  assert_eq!(throttle.attempt(&mut boot,30,id(3),id(4),launch(id(4)),settings())
    .unwrap().desktop,Some(id(4)));
 }
 #[test]fn failed_identity_check_never_consumes_restart_window(){
  let mut boot=running();let mut throttle=RestartGate::new(20);
  assert_eq!(throttle.attempt(&mut boot,60,id(2),id(3),launch(id(4)),settings()),
    Err(Error::State));
  assert_eq!(throttle.last_success(),None);
  assert!(throttle.attempt(&mut boot,60,id(2),id(3),launch(id(3)),settings()).is_ok());
 }
 #[test]fn monotonic_clock_regression_fails_closed(){
  let mut boot=running();let mut throttle=RestartGate::new(8);
  throttle.attempt(&mut boot,100,id(2),id(3),launch(id(3)),settings()).unwrap();
  assert_eq!(throttle.attempt(&mut boot,99,id(3),id(4),launch(id(4)),settings()),
    Err(Error::State));
  assert_eq!(boot.health().desktop,Some(id(3)));
 }
}
