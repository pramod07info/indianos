//! M1: authenticated desktop workspace controlling real persistent VFS I/O.
//! The existing R031.9 QEMU desktop runs real ring-3 apps separately; this
//! Rust integration is the next service API, not the CPU/syscall wiring itself.
use crate::block::{BlockDevice,BlockWindow};
use crate::m1_partition::{self,MountError};
use crate::hier_vfs::HierVfs;
use crate::batch5_session::Identity;
use crate::batch6_boot::{Bootstrap,Error as BootError};
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE,VIEW,Error as AccessError};
use crate::batch6_surface::{Surfaces,Error as SurfaceError,Damage};
use crate::batch2_launcher::Launch;
use crate::batch2_settings::Settings;
use crate::batch2_windows::{Window,Rect};
use crate::m1_persistent_terminal::{self,Error as TerminalError};
use crate::m1_input::{InputLine,Event as KeyEvent,Error as KeyError,MAX_LINE};

/// PS/2 set-1 unshifted ASCII subset for trusted QEMU keyboard events.
/// A full keymap, modifiers, Unicode and independent compositor remain M1 work.
fn ascii_from_set1(scan:u16,shift:bool)->Option<u8>{
 let basic=match scan{
  0x1e=>b'a',0x30=>b'b',0x2e=>b'c',0x20=>b'd',
  0x12=>b'e',0x21=>b'f',0x22=>b'g',0x23=>b'h',
  0x17=>b'i',0x24=>b'j',0x25=>b'k',0x26=>b'l',
  0x32=>b'm',0x31=>b'n',0x18=>b'o',0x19=>b'p',
  0x10=>b'q',0x13=>b'r',0x1f=>b's',0x14=>b't',
  0x16=>b'u',0x2f=>b'v',0x11=>b'w',0x2d=>b'x',
  0x15=>b'y',0x2c=>b'z',
  0x02=>b'1',0x03=>b'2',0x04=>b'3',0x05=>b'4',
  0x06=>b'5',0x07=>b'6',0x08=>b'7',0x09=>b'8',
  0x0a=>b'9',0x0b=>b'0',
  0x39=>b' ',0x35=>b'/',0x34=>b'.',0x0c=>b'-',
  0x0d=>b'=',0x1c=>b'\n',0x0e=>8,
  _=>return None,
 };
 if !shift{return Some(basic);}
 Some(match basic{
  b'a'..=b'z'=>basic.to_ascii_uppercase(),
  b'1'=>b'!',b'2'=>b'@',b'3'=>b'#',b'4'=>b'$',
  b'5'=>b'%',b'6'=>b'^',b'7'=>b'&',b'8'=>b'*',
  b'9'=>b'(',b'0'=>b')',b'-'=>b'_',b'='=>b'+',
  b'/'=>b'?',b'.'=>b'>',
  _=>basic,
 })
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 State,Boot(BootError),Auth(AccessError),
 Terminal(TerminalError),Graphics(SurfaceError),Keyboard(KeyError)
}
pub struct Desktop<D:BlockDevice,const P:usize,const E:usize,
 const W:usize,const K:usize,const G:usize,const Q:usize>{
 pub boot:Bootstrap<P,E,W,K>,
 pub rights:Access<G>,
 pub volume:HierVfs<D>,
 pub surface:Surfaces<Q>,
 pub input:InputLine,
 owner:Option<Identity>,
 window:u32,
 // Authenticated PS/2 modifier state, reset on each desktop session.
 shift_left:bool,shift_right:bool,
 // Exactly one pending full-window damage is sufficient; more keystrokes
 // mutate the input model and the eventual draw observes the latest state.
 redraw_pending:bool,
}
impl<D:BlockDevice,const P:usize,const E:usize,const W:usize,
 const K:usize,const G:usize,const Q:usize> Desktop<D,P,E,W,K,G,Q>{
 /// Requires an already mounted and validated persistent volume; never
 /// format/probe host or guest disks as a side effect.
 pub fn from_mounted(volume:HierVfs<D>)->Self{
  Self{boot:Bootstrap::new(),rights:Access::new(),volume,
   surface:Surfaces::new(),input:InputLine::new(),owner:None,
   window:0,shift_left:false,shift_right:false,redraw_pending:false}
 }
 pub fn start(&mut self,init:Identity,init_image:Launch,
  user:Identity,user_image:Launch,settings:Settings,
  window:u32,expires:u64)->Result<(),Error>{
  if self.owner.is_some()||init==user||window==0||expires==0||
     init.pid==0||user.pid==0||init.pid==user.pid||
     init.generation==0||user.generation==0||
     init_image.pid!=init.pid||init_image.generation!=init.generation||
     user_image.pid!=user.pid||user_image.generation!=user.generation||
     P<2||W==0||K==0||G==0||Q==0{
   return Err(Error::State);
  }
  // Validate BOTH signed launch descriptors before mutating the init
  // service graph. An invalid Desktop ELF must not strand PID1 in
  // InitReady or prevent a subsequent clean startup attempt.
  let mut preflight=crate::batch2_launcher::LaunchQueue::<2>::new();
  preflight.stage(init_image)
    .map_err(|e|Error::Boot(BootError::Session(
      crate::batch5_session::SessionError::Launch(e))))?;
  preflight.stage(user_image)
    .map_err(|e|Error::Boot(BootError::Session(
      crate::batch5_session::SessionError::Launch(e))))?;
  self.boot.configure(11,22).map_err(Error::Boot)?;
  self.boot.start_init(init,init_image,settings,11).map_err(Error::Boot)?;
  self.boot.start_desktop(user,user_image,settings,22).map_err(Error::Boot)?;
  self.boot.session.create_window(user,Window{
   pid:user.pid,generation:user.generation,id:window,
   rect:Rect{x:16,y:16,w:640,h:480},
  }).map_err(|e|Error::Boot(BootError::Session(e)))?;
  self.rights.grant(user,VIEW|FILE_READ|FILE_WRITE,expires)
   .map_err(Error::Auth)?;
  self.boot.session.focus(user,window)
   .map_err(|e|Error::Boot(BootError::Session(e)))?;
  self.owner=Some(user);self.window=window;
  self.shift_left=false;self.shift_right=false;self.redraw_pending=false;
  // A newly started desktop needs its first frame even with no keystrokes.
  // The compositor consumes this owner-bound damage during normal startup.
  self.request_redraw(user)?;
  Ok(())
 }
 /// Coalesce full-window updates until the compositor drains the pending
 /// event. Input remains bounded by InputLine; pending damage is owned by
 /// the authenticated desktop process and never grants framebuffer access.
 fn request_redraw(&mut self,caller:Identity)->Result<(),Error>{
  if !self.redraw_pending{
   self.surface.submit(&self.boot.session.windows,caller,self.window,
    Rect{x:0,y:0,w:640,h:480}).map_err(Error::Graphics)?;
   self.redraw_pending=true;
  }
  Ok(())
 }
 /// All methods require the kernel to bind caller to actual running PCB.
 pub fn execute(&mut self,caller:Identity,now:u64,
  input:&[u8],output:&mut[u8])->Result<usize,Error>{
  if self.owner!=Some(caller)||self.boot.desktop_identity()!=Some(caller)
   ||!self.boot.session.owns(caller){return Err(Error::State);}
  self.rights.require(caller,VIEW,now).map_err(Error::Auth)?;
  // Persistence happens at block-backed VFS, not Batch 7's ephemeral RAMFs.
  let n=m1_persistent_terminal::command(&mut self.volume,&self.rights,
    caller,now,input,output).map_err(Error::Terminal)?;
  self.request_redraw(caller)?;
  Ok(n)
 }
 /// Trusted kernel input broker must supply ASCII after scancode mapping
 /// and caller identity from the scheduler, never directly from an app.
 /// Press Enter to execute the complete command against persistent storage.
 pub fn key(&mut self,caller:Identity,now:u64,key:u8,
  out:&mut[u8])->Result<Option<usize>,Error>{
  if self.owner!=Some(caller)||self.boot.desktop_identity()!=Some(caller)||
   !self.boot.session.owns(caller){return Err(Error::State);}
  self.rights.require(caller,VIEW,now).map_err(Error::Auth)?;
  let mut line=[0u8;MAX_LINE];
  match self.input.feed(key,&mut line).map_err(Error::Keyboard)?{
   KeyEvent::Submitted(n)=>{
    match self.execute(caller,now,&line[..n],out){
     Ok(n)=>Ok(Some(n)),
     Err(error)=>{
      // Only Terminal-level failures are safely retryable. A graphical
      // redraw error might occur AFTER the file write actually committed.
      if matches!(error,Error::Terminal(_)){
       for &ch in &line[..n]{
        self.input.feed(ch,&mut[]).map_err(Error::Keyboard)?;
       }
       self.request_redraw(caller)?;
      }
      Err(error)
     }
    }
   },
   KeyEvent::Typed|KeyEvent::Erased=>{
    self.request_redraw(caller)?;
    Ok(None)
   },
   KeyEvent::Empty=>Ok(None),
  }
 }
 /// A trusted kernel PS/2 ingress source supplies an actual set-1
 /// scancode and edge. Focus, PID and generation are resolved through
 /// the session's authenticated hardware input queue, never user registers.
 /// The actual x86 IRQ hookup and userspace compositor remain pending.
 pub fn physical_key(&mut self,now:u64,scan:u16,pressed:bool,
  out:&mut[u8])->Result<Option<usize>,Error>{
  let owner=self.owner.ok_or(Error::State)?;
  if self.boot.desktop_identity()!=Some(owner){return Err(Error::State);}
  self.boot.session.physical_key(scan,pressed)
    .map_err(|e|Error::Boot(BootError::Session(e)))?;
  let event=self.boot.session.receive_key(owner)
    .map_err(|e|Error::Boot(BootError::Session(e)))?;
  if event.focus.pid!=owner.pid||event.focus.generation!=owner.generation||
     event.focus.window!=self.window{return Err(Error::State);}
  // Only trusted, focused PS/2 edges may modify keyboard state.
  match event.scan_code{
   0x2a=>{self.shift_left=event.pressed;return Ok(None);},
   0x36=>{self.shift_right=event.pressed;return Ok(None);},
   _=>{},
  }
  if !event.pressed{return Ok(None);}
  let ch=ascii_from_set1(event.scan_code,self.shift_left||self.shift_right)
    .ok_or(Error::Keyboard(KeyError::Invalid))?;
  self.key(owner,now,ch,out)
 }
 pub fn next_draw(&mut self)->Result<Damage,Error>{
  let damage=self.surface.next().map_err(Error::Graphics)?;
  if self.owner==Some(damage.owner)&&damage.window==self.window{
   self.redraw_pending=false;
  }
  Ok(damage)
 }
 /// Trusted scheduler call when the desktop process faults or exits.
 /// Preserve committed filesystem objects while clearing ALL transient UI
 /// ownership, focus, modifier keys, buffered commands and damage events.
 pub fn desktop_crashed(&mut self,identity:Identity)->Result<(),Error>{
  if self.owner!=Some(identity){return Err(Error::State);}
  self.boot.process_exited(identity).map_err(Error::Boot)?;
  self.surface.retire(identity);
  self.rights.revoke(identity).map_err(Error::Auth)?;
  self.input.reset();
  self.owner=None;
  self.window=0;
  self.shift_left=false;self.shift_right=false;self.redraw_pending=false;
  Ok(())
 }
 /// Install the new, scheduler-authenticated desktop image after a crash.
 /// PID1 must still be live; Bootstrap enforces a finite restart budget.
 /// This is the service-model bridge, not hardware IRQ or bootloader wiring.
 pub fn restart_desktop(&mut self,identity:Identity,launch:Launch,
  settings:Settings,window:u32,expires:u64)->Result<(),Error>{
  if self.owner.is_some()||window==0||expires==0||G==0||Q==0||
     identity.pid==0||identity.generation==0||
     launch.pid!=identity.pid||launch.generation!=identity.generation{
    return Err(Error::State);
  }
  self.boot.recover_desktop(identity,launch,settings).map_err(Error::Boot)?;
  let result=(||{
   self.boot.session.create_window(identity,Window{
    pid:identity.pid,generation:identity.generation,id:window,
    rect:Rect{x:16,y:16,w:640,h:480},
   }).map_err(|e|Error::Boot(BootError::Session(e)))?;
   self.rights.grant(identity,VIEW|FILE_READ|FILE_WRITE,expires)
     .map_err(Error::Auth)?;
   self.boot.session.focus(identity,window)
     .map_err(|e|Error::Boot(BootError::Session(e)))?;
   Ok::<(),Error>(())
  })();
  if let Err(error)=result{
   let _=self.rights.revoke(identity);
   let _=self.boot.process_exited(identity);
   return Err(error);
  }
  self.owner=Some(identity);self.window=window;
  self.input.reset();self.shift_left=false;self.shift_right=false;
  self.redraw_pending=false;
  self.request_redraw(identity)?;
  Ok(())
 }
 /// Authenticated orderly shutdown: terminate the graphical session,
 /// revoke all window/file/input grants and retire PID1, leaving the mounted
 /// durable volume unchanged for a later reboot. Does not power off hardware.
 pub fn shutdown(&mut self,init:Identity)->Result<(),Error>{
  if init.pid==0||init.generation==0||self.owner==Some(init)||
     !self.boot.session.owns(init)||self.boot.phase()==crate::batch6_boot::Phase::Stopped{
   return Err(Error::State);
  }
  if let Some(desktop)=self.owner{
   self.exit(desktop)?;
  }
  self.boot.process_exited(init).map_err(Error::Boot)?;
  Ok(())
 }
 pub fn exit(&mut self,caller:Identity)->Result<(),Error>{
  if self.owner!=Some(caller){return Err(Error::State);}
  self.surface.retire(caller);
  self.rights.revoke(caller).map_err(Error::Auth)?;
  self.boot.process_exited(caller).map_err(Error::Boot)?;
  self.input.reset();self.owner=None;self.window=0;
  self.shift_left=false;self.shift_right=false;
  self.redraw_pending=false;Ok(())
 }
 /// Only use this after quiescing the desktop and device: the returned
 /// device retains its committed file data for remount after reboot.
 pub fn into_device(self)->D{self.volume.into_device()}
}

/// Construct a desktop over an explicitly selected, previously formatted
/// MBR/GPT partition. This never initializes or reformats a real disk.
impl<'a,D:BlockDevice,const P:usize,const E:usize,const W:usize,
 const K:usize,const G:usize,const Q:usize>
 Desktop<BlockWindow<'a,D>,P,E,W,K,G,Q>{
 pub fn mount_selected(disk:&'a mut D,index:u32)->Result<Self,MountError>{
  m1_partition::mount(disk,index).map(Self::from_mounted)
 }
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 const SECTORS:usize=100;
 struct Disk{data:[[u8;SECTOR_BYTES];SECTORS]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{SECTORS as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*from;Ok(())
  }
 }
 fn user(pid:u32)->Identity{Identity{pid,generation:1}}
 fn load(pid:u32)->Launch{Launch{pid,generation:1,image:Image{
  image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 type TestDesk=Desktop<Disk,3,3,3,3,3,5>;
 #[test]fn desktop_file_created_and_visible_after_real_vfs_remount(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let v=HierVfs::format_blank(disk).unwrap();
  let mut gui=TestDesk::from_mounted(v);
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  assert_eq!(gui.execute(user(2),1,b"mkdir /home",&mut out),Ok(7));
  assert_eq!(gui.execute(user(2),1,b"write /home/note IndianOS",&mut out),Ok(5));
  assert_eq!(gui.execute(user(2),1,b"cat /home/note",&mut out),Ok(8));
  assert_eq!(&out[..8],b"IndianOS");
  assert_eq!(gui.next_draw().unwrap().owner,user(2));
  assert_eq!(gui.next_draw(),Err(Error::Graphics(SurfaceError::Empty)));
  assert!(gui.execute(user(3),1,b"cat /home/note",&mut out).is_err());
  gui.exit(user(2)).unwrap();
  assert!(gui.execute(user(2),1,b"ls /home",&mut out).is_err());
  let disk=gui.into_device();
  let v=HierVfs::mount(disk).unwrap();
  let mut resumed=TestDesk::from_mounted(v);
  resumed.start(user(1),load(1),user(2),load(2),settings(),9,100).unwrap();
  assert_eq!(resumed.execute(user(2),1,b"cat /home/note",&mut out),Ok(8));
  assert_eq!(&out[..8],b"IndianOS");
 }
 #[test]fn r039_failed_terminal_command_keeps_editable_input(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  for &ch in b"mkdir /home"{gui.key(user(2),1,ch,&mut[]).unwrap();}
  let mut short=[0xaau8;2];
  assert_eq!(gui.key(user(2),1,b'\n',&mut short),
    Err(Error::Terminal(TerminalError::OutputTooSmall)));
  assert_eq!(gui.input.length(),11);
  assert_eq!(short,[0xaa;2]);
  let mut out=[0u8;256];
  assert_eq!(gui.key(user(2),1,b'\n',&mut out),Ok(Some(7)));
  assert_eq!(gui.input.length(),0);
 }
 #[test]fn authenticated_keyboard_enters_persistent_commands(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let v=HierVfs::format_blank(disk).unwrap();
  let mut gui=TestDesk::from_mounted(v);
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  for &key in b"mkdir /home"{
   assert_eq!(gui.key(user(2),1,key,&mut out),Ok(None));
  }
  assert_eq!(gui.key(user(2),1,b'\n',&mut out),Ok(Some(7)));
  for &key in b"write /home/plan hello"{
   assert_eq!(gui.key(user(2),1,key,&mut out),Ok(None));
  }
  assert!(matches!(gui.key(user(3),1,b'\n',&mut out),
    Err(Error::State)));
  assert_eq!(gui.key(user(2),1,b'\n',&mut out),Ok(Some(5)));
  let disk=gui.into_device();
  let mut fs=HierVfs::mount(disk).unwrap();
  let mut bytes=[0u8;32];
  assert_eq!(fs.read("/home/plan",&mut bytes),Ok(5));
  assert_eq!(&bytes[..5],b"hello");
 }
 #[test]fn m1_terminal_keystrokes_redraw_without_queue_overflow(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  // Far more than Q=5 events: only one redraw remains queued.
  for &key in b"mkdir /docs"{
   assert_eq!(gui.key(user(2),1,key,&mut out),Ok(None));
  }
  assert_eq!(gui.next_draw().unwrap().window,8);
  assert_eq!(gui.next_draw(),Err(Error::Graphics(SurfaceError::Empty)));
  assert_eq!(gui.key(user(2),1,b'\n',&mut out),Ok(Some(7)));
  assert_eq!(gui.next_draw().unwrap().window,8);
  for &key in b"write /docs/note wrongx"{
   assert_eq!(gui.key(user(2),1,key,&mut out),Ok(None));
  }
  assert_eq!(gui.key(user(2),1,8,&mut out),Ok(None));
  assert_eq!(gui.key(user(2),1,b'\n',&mut out),Ok(Some(5)));
  assert_eq!(gui.next_draw().unwrap().window,8);
  assert_eq!(gui.next_draw(),Err(Error::Graphics(SurfaceError::Empty)));
  let mut bytes=[0u8;32];
  assert_eq!(gui.volume.read("/docs/note",&mut bytes),Ok(5));
  assert_eq!(&bytes[..5],b"wrong");
 }
 #[test]fn m1_desktop_mounts_explicit_partition_without_touching_neighbors(){
  const N:usize=160;
  struct PartitionDisk{data:[[u8;SECTOR_BYTES];N]}
  impl BlockDevice for PartitionDisk{
   fn sector_count(&self)->u64{N as u64}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
   }
   fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*from;Ok(())
   }
  }
  let mut disk=PartitionDisk{data:[[0;SECTOR_BYTES];N]};
  disk.data[0][510]=0x55;disk.data[0][511]=0xaa;
  disk.data[0][450]=0x83;
  disk.data[0][454..458].copy_from_slice(&16u32.to_le_bytes());
  disk.data[0][458..462].copy_from_slice(&100u32.to_le_bytes());
  assert!(Desktop::<crate::block::BlockWindow<'_,PartitionDisk>,3,3,3,3,3,5>
   ::mount_selected(&mut disk,0).is_err()); // never autoformats
  {
   let view=BlockWindow::new(&mut disk,16,100).unwrap();
   let fs=HierVfs::format_blank(view).unwrap();
   drop(fs);
  }
  {
   let mut desk=Desktop::<crate::block::BlockWindow<'_,PartitionDisk>,3,3,3,3,3,5>
    ::mount_selected(&mut disk,0).unwrap();
   desk.start(user(1),load(1),user(2),load(2),settings(),10,100).unwrap();
   let mut output=[0u8;256];
   assert_eq!(desk.execute(user(2),1,b"mkdir /docs",&mut output),Ok(7));
   assert_eq!(desk.execute(user(2),1,b"write /docs/x persistent",&mut output),Ok(5));
   desk.exit(user(2)).unwrap();
  }
  {
   let mut fs=crate::m1_partition::mount(&mut disk,0).unwrap();
   let mut output=[0u8;32];
   assert_eq!(fs.read("/docs/x",&mut output),Ok(10));
   assert_eq!(&output[..10],b"persistent");
  }
  assert_eq!(disk.data[0][510..512],[0x55,0xaa]);
  assert_eq!(disk.data[15],[0;SECTOR_BYTES]);
  assert_eq!(disk.data[116],[0;SECTOR_BYTES]);
 }
 #[test]fn m1_invalid_desktop_elf_cannot_partially_start_pid1(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  let mut invalid=load(2);invalid.image.verified=false;
  assert_eq!(gui.start(user(1),load(1),user(2),invalid,
    settings(),8,99),
    Err(Error::Boot(BootError::Session(
      crate::batch5_session::SessionError::Launch(
        crate::batch2_launcher::LaunchError::Unverified)))));
  assert_eq!(gui.boot.phase(),crate::batch6_boot::Phase::Cold);
  assert_eq!(gui.boot.desktop_identity(),None);
  let mut mismatched=load(2);mismatched.generation=3;
  assert_eq!(gui.start(user(1),load(1),user(2),mismatched,
    settings(),8,99),Err(Error::State));
  assert_eq!(gui.boot.phase(),crate::batch6_boot::Phase::Cold);
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  assert_eq!(gui.boot.desktop_identity(),Some(user(2)));
 }
 #[test]fn m1_physical_scancode_routes_only_to_focused_authenticated_terminal(){
  let disk=Disk{data:[[0;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  for scan in [0x32,0x25,0x20,0x17,0x13,0x39,0x35,
               0x23,0x18,0x32,0x12]{
   assert_eq!(gui.physical_key(1,scan,false,&mut out),Ok(None));
   assert_eq!(gui.physical_key(1,scan,true,&mut out),Ok(None));
  }
  assert_eq!(gui.physical_key(1,0x1c,true,&mut out),Ok(Some(7)));
  assert_eq!(&out[..7],b"created");
  let mut names=[0u8;256];
  assert_eq!(gui.volume.list_dir("/",&mut names),Ok(7));
  assert_eq!(&names[..7],b"d:home\n");
  assert_eq!(gui.physical_key(1,0,true,&mut out),
    Err(Error::Boot(BootError::Session(
      crate::batch5_session::SessionError::Input(
       crate::batch2_keyboard::InputError::Invalid)))));
 }
 #[test]fn m1_keyboard_retires_with_desktop_and_rejects_unmapped_scancode(){
  let disk=Disk{data:[[0;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;32];
  assert_eq!(gui.physical_key(1,0x3b,true,&mut out),
    Err(Error::Keyboard(KeyError::Invalid))); // function key not ASCII
  assert_eq!(gui.input.length(),0);
  gui.exit(user(2)).unwrap();
  assert_eq!(gui.physical_key(1,0x1e,true,&mut out),Err(Error::State));
  assert_eq!(gui.next_draw(),Err(Error::Graphics(SurfaceError::Empty)));
 }
 #[test]fn expired_rights_prevent_file_writes(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,10).unwrap();
  let mut out=[0u8;256];
  assert_eq!(gui.execute(user(2),10,b"mkdir /home",&mut out),
   Err(Error::Auth(AccessError::Expired)));
  assert_eq!(gui.volume.generation(),1);
 }
 #[test]fn m1_ps2_shifted_text_reaches_persistent_terminal_and_remounts(){
  let disk=Disk{data:[[0;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  for scan in [0x32,0x25,0x20,0x17,0x13,0x39,0x35,
               0x23,0x18,0x32,0x12,0x1c]{
   gui.physical_key(1,scan,true,&mut out).unwrap();
  }
  // "write /home/msg Hello": Shift applies only to the first letter.
  for scan in [0x11,0x13,0x17,0x14,0x12,0x39,0x35,0x23,
               0x18,0x32,0x12,0x35,0x32,0x1f,0x22,0x39]{
   gui.physical_key(1,scan,true,&mut out).unwrap();
  }
  assert_eq!(gui.physical_key(1,0x2a,true,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x23,true,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x2a,false,&mut out),Ok(None));
  for scan in [0x12,0x26,0x26,0x18]{
   gui.physical_key(1,scan,true,&mut out).unwrap();
  }
  assert_eq!(gui.physical_key(1,0x1c,true,&mut out),Ok(Some(5)));
  let disk=gui.into_device();
  let mut fs=HierVfs::mount(disk).unwrap();
  let mut data=[0u8;32];
  assert_eq!(fs.read("/home/msg",&mut data),Ok(5));
  assert_eq!(&data[..5],b"Hello");
 }
 #[test]fn m1_ps2_shift_edges_do_not_type_or_leak_between_sessions(){
  let disk=Disk{data:[[0;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  assert_eq!(gui.physical_key(1,0x2a,true,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x36,true,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x2a,false,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x1e,true,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x36,false,&mut out),Ok(None));
  assert_eq!(gui.physical_key(1,0x30,true,&mut out),Ok(None));
  assert_eq!(gui.input.length(),2);
  gui.exit(user(2)).unwrap();
  assert_eq!(gui.input.length(),0);
  let disk=gui.into_device();
  let mut resumed=TestDesk::from_mounted(HierVfs::mount(disk).unwrap());
  resumed.start(user(1),load(1),user(2),load(2),settings(),9,100).unwrap();
  assert_eq!(resumed.physical_key(1,0x2e,true,&mut out),Ok(None));
  assert_eq!(resumed.input.length(),1);
  assert_eq!(resumed.physical_key(1,0x1c,false,&mut out),Ok(None));
 }


 #[test]fn desktop_crash_restart_preserves_files_and_revokes_old_inputs(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  let mut out=[0u8;256];
  gui.execute(user(2),1,b"write /note survived",&mut out).unwrap();
  gui.key(user(2),1,b'a',&mut out).unwrap();
  gui.desktop_crashed(user(2)).unwrap();
  assert_eq!(gui.execute(user(2),1,b"cat /note",&mut out),Err(Error::State));
  let next=Identity{pid:2,generation:2};
  let mut new_launch=load(2);new_launch.generation=2;
  gui.restart_desktop(next,new_launch,settings(),9,100).unwrap();
  assert_eq!(gui.execute(next,1,b"cat /note",&mut out),Ok(8));
  assert_eq!(&out[..8],b"survived");
  assert_eq!(gui.execute(user(2),1,b"cat /note",&mut out),Err(Error::State));
  assert_eq!(gui.input.length(),0);
  let damage=gui.next_draw().unwrap();
  assert_eq!(damage.owner,next);
  assert_eq!(damage.window,9);
 }
 #[test]fn invalid_desktop_restart_cannot_steal_old_files_or_focus(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut gui=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  gui.start(user(1),load(1),user(2),load(2),settings(),8,99).unwrap();
  gui.desktop_crashed(user(2)).unwrap();
  let next=Identity{pid:2,generation:2};
  let mut bad=load(2);bad.generation=2;bad.image.verified=false;
  assert!(gui.restart_desktop(next,bad,settings(),9,100).is_err());
  assert!(!gui.boot.session.owns(next));
  let mut proper=load(2);proper.generation=2;
  assert_eq!(gui.restart_desktop(next,proper,settings(),9,100),Ok(()));
  assert_eq!(gui.restart_desktop(next,proper,settings(),10,100),Err(Error::State));
  assert_eq!(gui.desktop_crashed(user(2)),Err(Error::State));
 }


 #[test]fn graphical_desktop_presents_initial_frame_before_any_input(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut desk=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  desk.start(user(1),load(1),user(2),load(2),settings(),8,50).unwrap();
  let frame=desk.next_draw().unwrap();
  assert_eq!(frame.owner,user(2));assert_eq!(frame.window,8);
  assert_eq!(desk.next_draw(),Err(Error::Graphics(SurfaceError::Empty)));
 }
 #[test]fn clean_shutdown_revokes_desktop_and_preserves_user_files(){
  let disk=Disk{data:[[0u8;SECTOR_BYTES];SECTORS]};
  let mut desk=TestDesk::from_mounted(HierVfs::format_blank(disk).unwrap());
  desk.start(user(1),load(1),user(2),load(2),settings(),8,100).unwrap();
  let mut out=[0u8;256];
  desk.execute(user(2),1,b"write /note survives",&mut out).unwrap();
  assert_eq!(desk.shutdown(user(2)),Err(Error::State));
  assert_eq!(desk.shutdown(Identity{pid:1,generation:2}),Err(Error::State));
  assert_eq!(desk.shutdown(user(1)),Ok(()));
  assert_eq!(desk.boot.phase(),crate::batch6_boot::Phase::Stopped);
  assert_eq!(desk.execute(user(2),1,b"cat /note",&mut out),Err(Error::State));
  let disk=desk.into_device();
  let mut fs=HierVfs::mount(disk).unwrap();
  let mut buf=[0u8;32];
  assert_eq!(fs.read("/note",&mut buf),Ok(8));
  assert_eq!(&buf[..8],b"survives");
 }

}



/// R019: auth-routed disk-backed developer source editing. These methods
/// operate on the existing mounted persistent HierVfs. The GUI's text
/// painting/cursor controls and ring-3 IPC wiring remain separate.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum EditorRouteError {
 State,
 Permission(AccessError),
 Editor(crate::m1_disk_editor::Error),
}
impl<D:BlockDevice,const P:usize,const E:usize,const W:usize,
 const K:usize,const G:usize,const Q:usize> Desktop<D,P,E,W,K,G,Q> {
 fn admit_editor(&self,caller:Identity,now:u64,right:u32)
   ->Result<(),EditorRouteError>{
  if self.owner!=Some(caller)||self.boot.desktop_identity()!=Some(caller)||
     !self.boot.session.owns(caller){return Err(EditorRouteError::State);}
  self.rights.require(caller,right,now)
    .map_err(EditorRouteError::Permission)
 }
 /// Open existing verified durable source file under current Desktop owner.
 pub fn editor_open(&mut self,caller:Identity,now:u64,
    editor:&mut crate::m1_disk_editor::DiskEditor,path:&str)
    ->Result<crate::m1_disk_editor::Status,EditorRouteError>{
  self.admit_editor(caller,now,FILE_READ)?;
  editor.open(caller,&mut self.volume,path).map_err(EditorRouteError::Editor)
 }
 /// Apply a complete insertion/deletion/replacement to an open buffer.
 /// This is a change to the working copy; explicit editor_save persists it.
 pub fn editor_replace(&mut self,caller:Identity,now:u64,
    editor:&mut crate::m1_disk_editor::DiskEditor,
    start:usize,end:usize,bytes:&[u8])->Result<usize,EditorRouteError>{
  self.admit_editor(caller,now,FILE_WRITE)?;
  editor.replace(caller,start,end,bytes).map_err(EditorRouteError::Editor)
 }
 pub fn editor_save(&mut self,caller:Identity,now:u64,
    editor:&mut crate::m1_disk_editor::DiskEditor)
    ->Result<u64,EditorRouteError>{
  self.admit_editor(caller,now,FILE_WRITE)?;
  editor.save(caller,&mut self.volume).map_err(EditorRouteError::Editor)
 }
 /// Explicitly discard stale/dirty editor bytes and reload committed data.
 pub fn editor_reload(&mut self,caller:Identity,now:u64,
    editor:&mut crate::m1_disk_editor::DiskEditor)
    ->Result<crate::m1_disk_editor::Status,EditorRouteError>{
  self.admit_editor(caller,now,FILE_READ)?;
  editor.reload(caller,&mut self.volume).map_err(EditorRouteError::Editor)
 }
 pub fn editor_read(&self,caller:Identity,now:u64,
    editor:&crate::m1_disk_editor::DiskEditor,out:&mut[u8])
    ->Result<usize,EditorRouteError>{
  self.admit_editor(caller,now,FILE_READ)?;
  editor.read(caller,out).map_err(EditorRouteError::Editor)
 }
 pub fn editor_close(&self,caller:Identity,now:u64,
    editor:&mut crate::m1_disk_editor::DiskEditor,discard:bool)
    ->Result<(),EditorRouteError>{
  self.admit_editor(caller,now,FILE_READ)?;
  editor.close(caller,discard).map_err(EditorRouteError::Editor)
 }
}



/// R019 WS01/WS03: authenticated project-tree CRUD on durable HierVfs.
/// Only the currently running Desktop identity and explicit VFS capability
/// can invoke these APIs; no direct untrusted application access is provided.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ProjectRouteError {
 Permission(EditorRouteError),
 Project(crate::m1_project_workspace::Error),
}
impl<D:BlockDevice,const P:usize,const E:usize,const W:usize,
 const K:usize,const G:usize,const Q:usize> Desktop<D,P,E,W,K,G,Q>{
 pub fn project_create(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,name:&str)
    ->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.create(caller,&mut self.volume,name).map_err(ProjectRouteError::Project)
 }
 pub fn project_open(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,name:&str)
    ->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_READ).map_err(ProjectRouteError::Permission)?;
  project.open(caller,&mut self.volume,name).map_err(ProjectRouteError::Project)
 }
 pub fn project_list(&mut self,caller:Identity,now:u64,
    project:&crate::m1_project_workspace::ProjectTree,out:&mut[u8])
    ->Result<usize,ProjectRouteError>{
  self.admit_editor(caller,now,FILE_READ).map_err(ProjectRouteError::Permission)?;
  project.list(caller,&mut self.volume,out).map_err(ProjectRouteError::Project)
 }
 pub fn project_refresh(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree)
    ->Result<u64,ProjectRouteError>{
  self.admit_editor(caller,now,FILE_READ).map_err(ProjectRouteError::Permission)?;
  project.refresh(caller,&mut self.volume).map_err(ProjectRouteError::Project)
 }
 pub fn project_create_file(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,
    name:&str,data:&[u8])->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.create_file(caller,&mut self.volume,name,data).map_err(ProjectRouteError::Project)
 }
 pub fn project_read_file(&mut self,caller:Identity,now:u64,
    project:&crate::m1_project_workspace::ProjectTree,
    name:&str,out:&mut[u8])->Result<usize,ProjectRouteError>{
  self.admit_editor(caller,now,FILE_READ).map_err(ProjectRouteError::Permission)?;
  project.read_file(caller,&mut self.volume,name,out).map_err(ProjectRouteError::Project)
 }
 pub fn project_save_file(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,
    name:&str,data:&[u8])->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.save_file(caller,&mut self.volume,name,data).map_err(ProjectRouteError::Project)
 }
 pub fn project_rename_file(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,
    old:&str,new:&str)->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.rename_file(caller,&mut self.volume,old,new).map_err(ProjectRouteError::Project)
 }
 pub fn project_delete_file(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,
    name:&str)->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.delete_file(caller,&mut self.volume,name).map_err(ProjectRouteError::Project)
 }
 pub fn project_rename(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree,
    name:&str)->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.rename_project(caller,&mut self.volume,name).map_err(ProjectRouteError::Project)
 }
 pub fn project_remove_empty(&mut self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree)
    ->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_WRITE).map_err(ProjectRouteError::Permission)?;
  project.remove_empty_project(caller,&mut self.volume).map_err(ProjectRouteError::Project)
 }
 pub fn project_close(&self,caller:Identity,now:u64,
    project:&mut crate::m1_project_workspace::ProjectTree)
    ->Result<(),ProjectRouteError>{
  self.admit_editor(caller,now,FILE_READ).map_err(ProjectRouteError::Permission)?;
  project.close(caller).map_err(ProjectRouteError::Project)
 }
}


#[cfg(test)]
mod r019_project_desktop_integration_tests {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 use crate::batch2_launcher::Image;
 use crate::batch2_settings::Theme;
 use crate::m1_project_workspace::ProjectTree;
 struct Disk{data:[[u8;SECTOR_BYTES];100]}
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{100}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;Ok(())
  }
 }
 type TestDesk=Desktop<Disk,3,3,2,2,3,8>;
 fn identity(pid:u32)->Identity{Identity{pid,generation:1}}
 fn image(pid:u32)->Launch{
  Launch{pid,generation:1,image:Image{
   image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}}
 }
 fn settings()->Settings{Settings{
  theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 #[test]fn real_project_operations_authorize_desktop_and_survive_remount(){
  let fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  let mut desktop=TestDesk::from_mounted(fs);
  desktop.start(identity(1),image(1),identity(2),image(2),
    settings(),8,50).unwrap();
  let mut project=ProjectTree::new();
  assert_eq!(desktop.project_create(identity(2),1,&mut project,"demo"),Ok(()));
  assert_eq!(desktop.project_create_file(identity(2),1,&mut project,"code",b"hello"),Ok(()));
  let mut result=[0u8;32];
  assert_eq!(desktop.project_read_file(identity(2),1,&project,"code",&mut result),Ok(5));
  assert_eq!(&result[..5],b"hello");
  let recycled=Identity{pid:2,generation:2};
  assert!(desktop.project_save_file(recycled,1,&mut project,"code",b"bad").is_err());
  assert_eq!(desktop.project_rename_file(identity(2),1,&mut project,"code","main"),Ok(()));
  assert_eq!(desktop.project_rename(identity(2),1,&mut project,"next"),Ok(()));
  let disk=desktop.into_device();
  let mut remounted=HierVfs::mount(disk).unwrap();
  let mut data=[0u8;32];
  assert_eq!(remounted.read("/work/next/main",&mut data),Ok(5));
  assert_eq!(&data[..5],b"hello");
 }
 #[test]fn expired_capability_rejects_mutation_before_disk_write(){
  let fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  let mut desktop=TestDesk::from_mounted(fs);
  desktop.start(identity(1),image(1),identity(2),image(2),
    settings(),8,5).unwrap();
  let mut project=ProjectTree::new();
  assert!(desktop.project_create(identity(2),5,&mut project,"demo").is_err());
  let mut listing=[0u8;32];
  assert!(desktop.volume.list_dir("/work/demo",&mut listing).is_err());
 }
}
