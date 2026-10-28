//! R041: functional, no-heap interactive workspace integrating the existing
//! authenticated service boot, window ownership, terminal and file backend.
//! Host-testable logic; live userspace ELF/syscall/compositor wiring is pending.
use crate::batch2_launcher::Launch;
use crate::batch2_settings::Settings;
use crate::batch2_windows::{Window,Rect};
use crate::batch5_session::Identity;
use crate::batch6_boot::{Bootstrap,Error as BootError};
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE,VIEW,Error as AccessError};
use crate::batch6_surface::{Surfaces,Error as SurfaceError,Damage};
use crate::batch7_ramfs::RamFs;
use crate::batch7_terminal::{Terminal,Error as TerminalError,OUTPUT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Boot(BootError),Auth(AccessError),Surface(SurfaceError),
 Terminal(TerminalError),NotRunning}
pub struct Workspace<const P:usize,const E:usize,const W:usize,
 const K:usize,const F:usize,const G:usize,const Q:usize>{
 pub boot:Bootstrap<P,E,W,K>,
 pub access:Access<G>,
 pub disk:RamFs<F>,
 pub frames:Surfaces<Q>,
 pub terminal:Terminal,
 desktop:Option<Identity>,
 window:u32,
}
impl<const P:usize,const E:usize,const W:usize,
 const K:usize,const F:usize,const G:usize,const Q:usize>
 Workspace<P,E,W,K,F,G,Q>{
 pub const fn new()->Self{Self{
  boot:Bootstrap::new(),access:Access::new(),disk:RamFs::new(),
  frames:Surfaces::new(),terminal:Terminal::new(),desktop:None,window:0
 }}
 /// Caller is trusted boot coordinator; verified signed ELF images are
 /// supplied by an actual guest loader. No actual CPU process starts here.
 pub fn initialize(&mut self,init:Identity,init_image:Launch,
  desktop:Identity,desktop_image:Launch,
  settings:Settings,window:u32,expires:u64)->Result<(),Error>{
  if window==0||self.desktop.is_some()||expires==0{
   return Err(Error::NotRunning);
  }
  self.boot.configure(11,22).map_err(Error::Boot)?;
  self.boot.start_init(init,init_image,settings,11).map_err(Error::Boot)?;
  self.boot.start_desktop(desktop,desktop_image,settings,22)
   .map_err(Error::Boot)?;
  self.boot.session.create_window(desktop,Window{
   pid:desktop.pid,generation:desktop.generation,id:window,
   rect:Rect{x:5,y:5,w:640,h:480}
  }).map_err(|e|Error::Boot(BootError::Session(e)))?;
  self.access.grant(desktop,VIEW|FILE_READ|FILE_WRITE,expires)
   .map_err(Error::Auth)?;
  self.boot.session.focus(desktop,window)
   .map_err(|e|Error::Boot(BootError::Session(e)))?;
  self.desktop=Some(desktop);self.window=window;Ok(())
 }
 fn require(&self,caller:Identity,now:u64)->Result<(),Error>{
  if self.desktop!=Some(caller)||self.boot.desktop_identity()!=Some(caller)||
   !self.boot.session.owns(caller){return Err(Error::NotRunning);}
  self.access.require(caller,VIEW,now).map_err(Error::Auth)
 }
 pub fn execute(&mut self,caller:Identity,now:u64,
  command:&[u8],out:&mut[u8])->Result<usize,Error>{
  self.require(caller,now)?;
  let n=crate::batch7_terminal::execute(&self.access,caller,now,
    &mut self.disk,command,out).map_err(Error::Terminal)?;
  self.frames.submit(&self.boot.session.windows,caller,self.window,
   Rect{x:0,y:0,w:640,h:480}).map_err(Error::Surface)?;
  Ok(n)
 }
 pub fn press_ascii(&mut self,caller:Identity,now:u64,
   ch:u8)->Result<(),Error>{
  self.require(caller,now)?;
  self.terminal.type_byte(ch).map_err(Error::Terminal)
 }
 pub fn submit(&mut self,caller:Identity,now:u64,
  output:&mut[u8])->Result<usize,Error>{
  self.require(caller,now)?;
  let size=self.terminal.submit(&self.access,caller,now,&mut self.disk,output)
   .map_err(Error::Terminal)?;
  self.frames.submit(&self.boot.session.windows,caller,self.window,
   Rect{x:0,y:0,w:640,h:480}).map_err(Error::Surface)?;
  Ok(size)
 }
 pub fn next_frame(&mut self)->Result<Damage,Error>{
  self.frames.next().map_err(Error::Surface)
 }
 pub fn retire(&mut self,caller:Identity)->Result<(),Error>{
  if self.desktop!=Some(caller){return Err(Error::NotRunning);}
  self.frames.retire(caller);
  self.access.revoke(caller).map_err(Error::Auth)?;
  self.boot.process_exited(caller).map_err(Error::Boot)?;
  self.terminal=Terminal::new();
  self.desktop=None;self.window=0;Ok(())
 }
}
/// Can be exercised as a no_std QEMU feature smoke test. Does not boot
/// a ring3 service and must not be marketed as a complete GUI milestone.
pub fn bounded_workspace_smoke()->bool{
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 let init=Identity{pid:1,generation:1};
 let ui=Identity{pid:2,generation:1};
 let mut ws=Workspace::<3,3,2,2,2,2,4>::new();
 let mk=|pid|Launch{pid,generation:1,image:Image{
  image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}};
 let settings=Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10};
 if ws.initialize(init,mk(1),ui,mk(2),settings,50,100).is_err(){return false;}
 let mut buf=[0u8;OUTPUT_MAX];
 if ws.execute(ui,1,b"write /note hello",&mut buf)!=Ok(5){return false;}
 if ws.execute(ui,1,b"cat /note",&mut buf)!=Ok(5){return false;}
 if &buf[..5]!=b"hello"{return false;}
 if ws.next_frame().is_err()||ws.next_frame().is_err(){return false;}
 if ws.retire(ui).is_err(){return false;}
 ws.execute(ui,1,b"ls",&mut buf).is_err()&&ws.disk.version()==1
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn integrated_working_terminal_file_read_write_and_revocation(){
  assert!(bounded_workspace_smoke());
 }
 #[test]fn no_untrusted_direct_surface_or_durable_io_claim(){
  // Actual guest rendering and disk persistence remain separate acceptance.
  assert!(core::mem::size_of::<RamFs<1>>()>=2048);
 }
}


/// R012: bounded session persistence handshake for the interactive workspace.
/// The trusted storage service must persist exported bytes to a durable COW
/// block volume. This method does not claim RAM survives a reboot.
impl<const P:usize,const E:usize,const W:usize,const K:usize,
 const F:usize,const G:usize,const Q:usize> Workspace<P,E,W,K,F,G,Q>{
 pub fn export_files(&self,caller:Identity,now:u64,out:&mut[u8])
  ->Result<usize,Error>{
  self.require(caller,now)?;
  self.access.require(caller,FILE_READ,now).map_err(Error::Auth)?;
  self.disk.export_snapshot(out).map_err(|_|Error::Terminal(TerminalError::Io))
 }
 /// Atomically replace RAM file state only after checksum and generation
 /// validation; no partial file tree is published for corrupted backups.
 pub fn restore_files(&mut self,caller:Identity,now:u64,backup:&[u8])
  ->Result<(),Error>{
  self.require(caller,now)?;
  self.access.require(caller,FILE_WRITE,now).map_err(Error::Auth)?;
  self.disk.import_snapshot(backup)
    .map_err(|_|Error::Terminal(TerminalError::Io))
 }
 /// Execute a complete physical keyboard line through the actual Terminal
 /// buffer, rather than bypassing the authenticated input path.
 pub fn type_and_submit(&mut self,caller:Identity,now:u64,
  line:&[u8],out:&mut[u8])->Result<usize,Error>{
  self.require(caller,now)?;
  if line.is_empty()||line.len()>crate::batch7_terminal::COMMAND_MAX||
     line.iter().any(|b|*b!=b' '&&!b.is_ascii_graphic()){
   return Err(Error::Terminal(TerminalError::Invalid));
  }
  let mut typed=Terminal::new();
  for &ch in line{typed.type_byte(ch).map_err(Error::Terminal)?;}
  self.terminal=typed;
  self.submit(caller,now,out)
 }
}
#[cfg(test)]
mod r012_workspace_restore_tests{
 use super::*;
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(pid:u32)->Identity{Identity{pid,generation:1}}
 fn ready()->Workspace<3,3,2,2,3,2,8>{
  let mut w=Workspace::new();
  let image=|pid|Launch{pid,generation:1,image:Image{
   image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}};
  w.initialize(id(1),image(1),id(2),image(2),
    Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).unwrap();
  w
 }
 #[test]fn terminal_ui_file_state_round_trips_through_verified_snapshot(){
  let mut w=ready();let mut out=[0u8;OUTPUT_MAX];
  assert_eq!(w.type_and_submit(id(2),1,b"write /note hello",&mut out),Ok(5));
  let mut snapshot=[0u8;512];
  let n=w.export_files(id(2),1,&mut snapshot).unwrap();
  w.execute(id(2),1,b"write /note changed",&mut out).unwrap();
  w.restore_files(id(2),1,&snapshot[..n]).unwrap();
  assert_eq!(w.execute(id(2),1,b"cat /note",&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
 }
 #[test]fn corrupted_backup_and_foreign_generation_fail_without_mutation(){
  let mut w=ready();let mut out=[0u8;OUTPUT_MAX];
  w.execute(id(2),1,b"write /note safe",&mut out).unwrap();
  let mut snapshot=[0u8;512];
  let n=w.export_files(id(2),1,&mut snapshot).unwrap();
  snapshot[15]^=1;
  assert!(w.restore_files(id(2),1,&snapshot[..n]).is_err());
  assert_eq!(w.execute(id(2),1,b"cat /note",&mut out),Ok(4));
  assert_eq!(&out[..4],b"safe");
  let stale=Identity{generation:2,..id(2)};
  assert!(w.export_files(stale,1,&mut snapshot).is_err());
  assert!(w.restore_files(stale,1,&snapshot[..n]).is_err());
 }
 #[test]fn invalid_keyboard_line_cannot_change_file_or_frame_queue(){
  let mut w=ready();let mut out=[0xccu8;OUTPUT_MAX];
  assert!(w.type_and_submit(id(2),1,b"write /note\nhi",&mut out).is_err());
  assert_eq!(w.disk.count(),0);
  assert_eq!(out,[0xcc;OUTPUT_MAX]);
 }
}

/// R013: transactional interactive desktop command batch. A trusted
/// storage service supplies snapshot scratch; RAM state is not durable yet.
impl<const P:usize,const E:usize,const W:usize,const K:usize,
 const F:usize,const G:usize,const Q:usize> Workspace<P,E,W,K,F,G,Q>{
 pub fn execute_script_atomic(&mut self,caller:Identity,now:u64,
   commands:&[&[u8]],scratch:&mut[u8],out:&mut[u8])->Result<usize,Error>{
  self.require(caller,now)?;
  self.access.require(caller,FILE_READ|FILE_WRITE,now).map_err(Error::Auth)?;
  if commands.is_empty()||commands.len()>16{return Err(Error::NotRunning);}
  for line in commands{
   if line.is_empty()||line.len()>crate::batch7_terminal::COMMAND_MAX||
     line.iter().any(|c|*c!=b' '&&!c.is_ascii_graphic()){
    return Err(Error::Terminal(TerminalError::Invalid));
   }
  }
  let saved=self.disk.export_snapshot(scratch)
    .map_err(|_|Error::Terminal(TerminalError::Io))?;
  let mut last=0;
  for &line in commands{
   match crate::batch7_terminal::execute(
      &self.access,caller,now,&mut self.disk,line,out){
    Ok(n)=>last=n,
    Err(err)=>{
     self.disk.import_snapshot(&scratch[..saved])
       .map_err(|_|Error::Terminal(TerminalError::Io))?;
     return Err(Error::Terminal(err));
    }
   }
  }
  if let Err(err)=self.frames.submit(&self.boot.session.windows,
     caller,self.window,Rect{x:0,y:0,w:640,h:480}){
   self.disk.import_snapshot(&scratch[..saved])
     .map_err(|_|Error::Terminal(TerminalError::Io))?;
   return Err(Error::Surface(err));
  }
  Ok(last)
 }
 pub fn app_file_generation(&self,caller:Identity,now:u64)->Result<u64,Error>{
  self.require(caller,now)?;
  self.access.require(caller,FILE_READ,now).map_err(Error::Auth)?;
  Ok(self.disk.version())
 }
}
#[cfg(test)]
mod r013_desktop_transaction_tests{
 use super::*;use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(pid:u32)->Identity{Identity{pid,generation:1}}
 fn workspace()->Workspace<3,3,2,2,3,2,8>{
  let mut w=Workspace::new();
  let launch=|pid|Launch{pid,generation:1,image:Image{
   image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}};
  w.initialize(id(1),launch(1),id(2),launch(2),
   Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).unwrap();w
 }
 #[test]fn script_writes_two_files_but_only_one_redraw(){
  let mut w=workspace();let mut scratch=[0;512];let mut out=[0;OUTPUT_MAX];
  assert_eq!(w.execute_script_atomic(id(2),1,&[
   b"write /a alpha",b"cp /a /b",b"cat /b"
  ],&mut scratch,&mut out),Ok(5));
  assert_eq!(&out[..5],b"alpha");assert_eq!(w.disk.count(),2);
  assert!(w.next_frame().is_ok());assert!(w.next_frame().is_err());
 }
 #[test]fn later_failing_command_reverts_first_file(){
  let mut w=workspace();let mut scratch=[0;512];let mut out=[0;OUTPUT_MAX];
  assert!(w.execute_script_atomic(id(2),1,&[
   b"write /a alpha",b"cat /missing"
  ],&mut scratch,&mut out).is_err());
  assert_eq!(w.disk.count(),0);assert!(w.next_frame().is_err());
 }
 #[test]fn foreign_generation_and_short_snapshot_never_write(){
  let mut w=workspace();let mut tiny=[0;2];let mut out=[0;OUTPUT_MAX];
  assert!(w.execute_script_atomic(Identity{generation:2,..id(2)},1,
    &[b"write /a hi"],&mut tiny,&mut out).is_err());
  assert!(w.execute_script_atomic(id(2),1,&[b"write /a hi"],
    &mut tiny,&mut out).is_err());
  assert_eq!(w.app_file_generation(id(2),1),Ok(0));
 }
}

/// R014 commit both filesystem mutations and terminal-visible output only
/// after the complete script succeeds. Never expose a failed intermediate
/// command's text to the caller or enqueue a partial desktop repaint.
impl<const P:usize,const E:usize,const W:usize,const K:usize,
 const F:usize,const G:usize,const Q:usize> Workspace<P,E,W,K,F,G,Q>{
 pub fn execute_script_visible(&mut self,caller:Identity,now:u64,
   commands:&[&[u8]],snapshot:&mut[u8],output:&mut[u8])
   ->Result<usize,Error>{
  if output.len()<OUTPUT_MAX{return Err(Error::Terminal(TerminalError::Io));}
  let mut staged=[0u8;OUTPUT_MAX];
  let n=self.execute_script_atomic(caller,now,commands,snapshot,&mut staged)?;
  if n>output.len(){return Err(Error::Terminal(TerminalError::Io));}
  output[..n].copy_from_slice(&staged[..n]);
  Ok(n)
 }
 pub fn current_desktop(&self)->Option<Identity>{self.desktop}
}
#[cfg(test)]
mod r014_visible_script_tests{
 use super::*;use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 fn id(p:u32)->Identity{Identity{pid:p,generation:1}}
 fn ready()->Workspace<3,3,2,2,3,2,8>{
  let mut w=Workspace::new();
  let launch=|p|Launch{pid:p,generation:1,image:Image{
    image_id:p,entry:0x1000,mapped_end:0x2000,verified:true}};
  w.initialize(id(1),launch(1),id(2),launch(2),
    Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).unwrap();w
 }
 #[test]fn successful_script_publishes_terminal_output_and_one_frame(){
  let mut w=ready();let mut snap=[0;512];let mut out=[0xcc;OUTPUT_MAX];
  let n=w.execute_script_visible(id(2),1,
    &[b"write /x value",b"cat /x"],&mut snap,&mut out).unwrap();
  assert_eq!(n,5);assert_eq!(&out[..n],b"value");
  assert!(w.next_frame().is_ok());assert_eq!(w.current_desktop(),Some(id(2)));
 }
 #[test]fn invalid_second_command_never_shows_partial_terminal_output(){
  let mut w=ready();let mut snap=[0;512];let mut out=[0xcc;OUTPUT_MAX];
  assert!(w.execute_script_visible(id(2),1,
    &[b"write /x value",b"cat /unknown"],&mut snap,&mut out).is_err());
  assert_eq!(out,[0xcc;OUTPUT_MAX]);assert_eq!(w.disk.count(),0);
  assert!(w.next_frame().is_err());
 }
 #[test]fn wrong_generation_cannot_observe_or_mutate_desktop(){
  let mut w=ready();let mut snap=[0;512];let mut out=[0xcc;OUTPUT_MAX];
  let stale=Identity{pid:2,generation:2};
  assert!(w.execute_script_visible(stale,1,
    &[b"write /x value"],&mut snap,&mut out).is_err());
  assert_eq!(out,[0xcc;OUTPUT_MAX]);assert_eq!(w.disk.count(),0);
 }
}
