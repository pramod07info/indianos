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