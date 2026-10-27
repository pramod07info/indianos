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
fn ascii_from_set1(scan:u16)->Option<u8>{
 Some(match scan{
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
   window:0,redraw_pending:false}
 }
 pub fn start(&mut self,init:Identity,init_image:Launch,
  user:Identity,user_image:Launch,settings:Settings,
  window:u32,expires:u64)->Result<(),Error>{
  if self.owner.is_some()||init==user||window==0||expires==0{
   return Err(Error::State);
  }
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
  self.owner=Some(user);self.window=window;self.redraw_pending=false;Ok(())
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
   KeyEvent::Submitted(n)=>self.execute(caller,now,&line[..n],out).map(Some),
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
  if !event.pressed{return Ok(None);}
  let ch=ascii_from_set1(event.scan_code).ok_or(Error::Keyboard(KeyError::Invalid))?;
  self.key(owner,now,ch,out)
 }
 pub fn next_draw(&mut self)->Result<Damage,Error>{
  let damage=self.surface.next().map_err(Error::Graphics)?;
  if self.owner==Some(damage.owner)&&damage.window==self.window{
   self.redraw_pending=false;
  }
  Ok(damage)
 }
 pub fn exit(&mut self,caller:Identity)->Result<(),Error>{
  if self.owner!=Some(caller){return Err(Error::State);}
  self.surface.retire(caller);
  self.rights.revoke(caller).map_err(Error::Auth)?;
  self.boot.process_exited(caller).map_err(Error::Boot)?;
  self.input.reset();self.owner=None;self.window=0;
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
}
