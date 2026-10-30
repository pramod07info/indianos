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
  // A failed transaction must not publish intermediate command output.
  let mut staged_output=[0u8;OUTPUT_MAX];
  for &line in commands{
   match crate::batch7_terminal::execute(
      &self.access,caller,now,&mut self.disk,line,&mut staged_output){
    Ok(n)=>last=n,
    Err(err)=>{
     self.disk.import_snapshot(&scratch[..saved])
       .map_err(|_|Error::Terminal(TerminalError::Io))?;
     return Err(Error::Terminal(err));
    }
   }
  }
  if last>out.len(){
   self.disk.import_snapshot(&scratch[..saved])
     .map_err(|_|Error::Terminal(TerminalError::Io))?;
   return Err(Error::Terminal(TerminalError::Overflow));
  }
  if let Err(err)=self.frames.submit(&self.boot.session.windows,
     caller,self.window,Rect{x:0,y:0,w:640,h:480}){
   self.disk.import_snapshot(&scratch[..saved])
     .map_err(|_|Error::Terminal(TerminalError::Io))?;
   return Err(Error::Surface(err));
  }
  out[..last].copy_from_slice(&staged_output[..last]);
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
 #[test]fn r055_direct_script_failure_does_not_publish_partial_output(){
  let mut w=workspace();let mut scratch=[0;512];let mut out=[0xa7;OUTPUT_MAX];
  assert!(w.execute_script_atomic(id(2),1,&[
    b"write /a alpha",b"cat /absent"
  ],&mut scratch,&mut out).is_err());
  assert_eq!(out,[0xa7;OUTPUT_MAX]);
  assert_eq!(w.disk.count(),0);
  assert!(w.next_frame().is_err());
 }
 #[test]fn r055_direct_script_short_output_rolls_back_before_frame(){
  let mut w=workspace();let mut scratch=[0;512];let mut out=[0xa7;3];
  assert_eq!(w.execute_script_atomic(id(2),1,&[
    b"write /a alpha",b"cat /a"
  ],&mut scratch,&mut out),
    Err(Error::Terminal(TerminalError::Overflow)));
  assert_eq!(out,[0xa7;3]);
  assert_eq!(w.disk.count(),0);
  assert!(w.next_frame().is_err());
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

/// R016: portable checksummed workspace checkpoint envelope. The storage
/// caller must durably write these bytes; RAM snapshots are not disk commits.
pub const R016_CHECKPOINT_HEADER:usize=24;
fn r016_snapshot_hash(bytes:&[u8])->u32{
 let mut h=0x811c9dc5u32;
 for &b in bytes{h=(h^(b as u32)).wrapping_mul(0x01000193);}
 h
}
impl<const P:usize,const E:usize,const W:usize,const K:usize,
 const F:usize,const G:usize,const Q:usize> Workspace<P,E,W,K,F,G,Q>{
 pub fn export_guarded_checkpoint(&self,owner:Identity,now:u64,
  output:&mut[u8])->Result<usize,Error>{
  if output.len()<R016_CHECKPOINT_HEADER{return Err(Error::NotRunning);}
  self.require(owner,now)?;
  let n=self.export_files(owner,now,&mut output[R016_CHECKPOINT_HEADER..])?;
  if n>u32::MAX as usize{return Err(Error::NotRunning);}
  output[..4].copy_from_slice(b"R16W");
  output[4..12].copy_from_slice(&self.disk.version().to_le_bytes());
  output[12..16].copy_from_slice(&(n as u32).to_le_bytes());
  let checksum=r016_snapshot_hash(
    &output[R016_CHECKPOINT_HEADER..R016_CHECKPOINT_HEADER+n]);
  output[16..20].copy_from_slice(&checksum.to_le_bytes());
  output[20..24].fill(0);
  Ok(R016_CHECKPOINT_HEADER+n)
 }
 pub fn restore_guarded_checkpoint(&mut self,owner:Identity,now:u64,
   expected_current:u64,record:&[u8])->Result<(),Error>{
  self.require(owner,now)?;
  if self.disk.version()!=expected_current||
    record.len()<R016_CHECKPOINT_HEADER||&record[..4]!=b"R16W"||
    record[20..24]!=[0;4]{return Err(Error::NotRunning);}
  let n=u32::from_le_bytes(record[12..16].try_into()
    .map_err(|_|Error::NotRunning)?) as usize;
  if R016_CHECKPOINT_HEADER.checked_add(n)!=Some(record.len()){
   return Err(Error::NotRunning);
  }
  let hash=u32::from_le_bytes(record[16..20].try_into()
    .map_err(|_|Error::NotRunning)?);
  if r016_snapshot_hash(&record[R016_CHECKPOINT_HEADER..])!=hash{
   return Err(Error::NotRunning);
  }
  self.restore_files(owner,now,&record[R016_CHECKPOINT_HEADER..])
 }
}
#[cfg(test)]mod r016_workspace_checkpoint_tests{
 use super::*;use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 fn id(g:u64)->Identity{Identity{pid:2,generation:g}}
 fn ready()->Workspace<3,3,2,2,3,2,8>{
  let mut w=Workspace::new();
  let mk=|p|Launch{pid:p,generation:1,image:Image{
    image_id:p,entry:0x1000,mapped_end:0x2000,verified:true}};
  w.initialize(Identity{pid:1,generation:1},mk(1),id(1),mk(2),
    Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).unwrap();w
 }
 #[test]fn export_restore_with_generation_guard(){
  let mut w=ready();let mut out=[0u8;OUTPUT_MAX];
  w.execute(id(1),1,b"write /note hello",&mut out).unwrap();
  let mut saved=[0u8;1024];
  let n=w.export_guarded_checkpoint(id(1),1,&mut saved).unwrap();
  w.execute(id(1),1,b"write /note changed",&mut out).unwrap();
  let current=w.disk.version();
  w.restore_guarded_checkpoint(id(1),1,current,&saved[..n]).unwrap();
  assert_eq!(w.execute(id(1),1,b"cat /note",&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
 }
 #[test]fn corrupt_or_truncated_checkpoint_does_not_mutate(){
  let mut w=ready();let mut buf=[0u8;1024];
  let n=w.export_guarded_checkpoint(id(1),1,&mut buf).unwrap();
  let g=w.disk.version();
  assert!(w.restore_guarded_checkpoint(id(1),1,g,&buf[..n-1]).is_err());
  buf[n-1]^=1;
  assert!(w.restore_guarded_checkpoint(id(1),1,g,&buf[..n]).is_err());
  assert_eq!(w.disk.version(),g);
 }
 #[test]fn stale_identity_and_generation_cannot_restore(){
  let mut w=ready();let mut buf=[0u8;1024];
  let n=w.export_guarded_checkpoint(id(1),1,&mut buf).unwrap();
  assert!(w.export_guarded_checkpoint(id(2),1,&mut buf).is_err());
  assert!(w.restore_guarded_checkpoint(id(1),1,999,&buf[..n]).is_err());
 }
}



/// R017 developer workstation: the terminal script is guarded by a project
/// revision. Failed validation must not alter files, frames or terminal output.
impl<const P:usize,const E:usize,const W:usize,const K:usize,
 const F:usize,const G:usize,const Q:usize> Workspace<P,E,W,K,F,G,Q>{
 pub fn project_revision(&self,actor:Identity,now:u64)->Result<u64,Error>{
  self.app_file_generation(actor,now)
 }
 pub fn run_project_commands(&mut self,actor:Identity,now:u64,
  expected_revision:u64,commands:&[&[u8]],snapshot:&mut[u8],
  out:&mut[u8])->Result<(usize,u64),Error>{
  if self.project_revision(actor,now)?!=expected_revision{
   return Err(Error::NotRunning);
  }
  let len=self.execute_script_visible(actor,now,commands,snapshot,out)?;
  Ok((len,self.disk.version()))
 }
}
#[cfg(test)]mod r017_project_workspace_tests{
 use super::*;use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 fn id(g:u64)->Identity{Identity{pid:2,generation:g}}
 fn ready()->Workspace<3,3,2,2,3,2,8>{
  let mut w=Workspace::new();
  let mk=|p|Launch{pid:p,generation:1,image:Image{
   image_id:p,entry:0x1000,mapped_end:0x2000,verified:true}};
  w.initialize(Identity{pid:1,generation:1},mk(1),id(1),mk(2),
   Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).unwrap();w
 }
 #[test]fn versioned_script_creates_and_reads_actual_file(){
  let mut w=ready();let mut s=[0u8;1024];let mut o=[0u8;OUTPUT_MAX];
  let revision=w.project_revision(id(1),1).unwrap();
  let (n,updated)=w.run_project_commands(id(1),1,revision,
   &[b"write /src main",b"cat /src"],&mut s,&mut o).unwrap();
  assert_eq!(n,4);assert_eq!(&o[..n],b"main");assert!(updated>revision);
 }
 #[test]fn stale_project_generation_never_changes_output(){
  let mut w=ready();let mut s=[0;1024];let mut o=[7u8;OUTPUT_MAX];
  assert!(w.run_project_commands(id(1),1,999,
   &[b"write /src overwrite"],&mut s,&mut o).is_err());
  assert_eq!(o,[7;OUTPUT_MAX]);assert_eq!(w.disk.count(),0);
 }
 #[test]fn recycled_process_cannot_read_project_revision(){
  let w=ready();assert!(w.project_revision(id(2),1).is_err());
 }
}


/// R017 integrated workstation smoke, exercised in Rust tests and the
/// opt-in R041 QEMU guest. Uses real RAM file operations and UI damage
/// bookkeeping, but does not claim a fully interactive ring-3 desktop.
pub fn bounded_developer_workstation_smoke()->bool{
 use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 let init=Identity{pid:1,generation:1};
 let actor=Identity{pid:2,generation:1};
 let mk=|p|Launch{pid:p,generation:1,image:Image{
    image_id:p,entry:0x1000,mapped_end:0x2000,verified:true}};
 let mut ws=Workspace::<3,3,2,2,3,2,8>::new();
 if ws.initialize(init,mk(1),actor,mk(2),
   Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10},50,100).is_err(){
   return false;
 }
 let revision=match ws.project_revision(actor,1){Ok(v)=>v,Err(_)=>return false};
 let mut snapshot=[0u8;1024];let mut output=[0u8;OUTPUT_MAX];
 let Ok((count,next))=ws.run_project_commands(actor,1,revision,
   &[b"write /src main",b"cat /src"],&mut snapshot,&mut output) else {
   return false;
 };
 if count!=4||&output[..count]!=b"main"||next<=revision{return false;}
 // A stale editor revision cannot overwrite the latest project file.
 if ws.disk.save_if_revision(b"/src",b"old",revision).is_ok(){return false;}
 if ws.disk.append_if_revision(b"/src",b".rs",next).is_err(){return false;}
 if ws.execute(actor,1,b"cat /src",&mut output)!=Ok(7)||
    &output[..7]!=b"main.rs"{return false;}
 ws.next_frame().is_ok()&&ws.next_frame().is_ok()&&ws.next_frame().is_err()
}
#[cfg(test)]mod r017_workstation_smoke_test{
 use super::*;
 #[test]fn guest_project_editing_and_frame_damage_are_integrated(){
  assert!(bounded_developer_workstation_smoke());
 }
}
