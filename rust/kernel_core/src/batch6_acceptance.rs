//! R040 Batch 6 combined contract acceptance.
//! This tests the Rust integration boundaries, NOT QEMU desktop execution,
//! actual driver DMA, durable media, signatures or independent release review.
#[cfg(test)]mod tests{
 use crate::batch6_boot::{Bootstrap,Phase};
 use crate::batch6_surface::Surfaces;
 use crate::batch6_security::{Access,VIEW,FILE_WRITE,SOCKET};
 use crate::batch6_storage::{self,AtomicFileBackend};
 use crate::batch6_network::Ports;
 use crate::batch6_devices::{self,Pci};
 use crate::batch6_recovery::Recovery;
 use crate::batch6_apps::{self,Requested,Format,Availability};
 use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 use crate::batch2_windows::{Window,Rect};
 use crate::batch5_session::Identity;

 fn id(pid:u32)->Identity{Identity{pid,generation:6}}
 fn launch(id:Identity)->Launch{Launch{pid:id.pid,generation:id.generation,
  image:Image{image_id:id.pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Light,pointer_speed:3,key_repeat:10}}
 struct Scratch{data:[u8;64],len:usize}
 impl AtomicFileBackend for Scratch{
  fn write_atomic(&mut self,path:&[u8],body:&[u8])->Result<(),()>{
   if path!=b"/home/config"||body.len()>64{return Err(());}
   self.data=[0;64];self.data[..body.len()].copy_from_slice(body);
   self.len=body.len();Ok(())
  }
  fn read_all(&mut self,path:&[u8],out:&mut[u8])->Result<usize,()>{
   if path!=b"/home/config"||out.len()<self.len{return Err(());}
   out[..self.len].copy_from_slice(&self.data[..self.len]);Ok(self.len)
  }
 }
 #[test]fn authenticated_desktop_to_storage_io_and_process_reap(){
  let mut boot=Bootstrap::<3,3,2,3>::new();
  boot.configure(11,22).unwrap();
  let owner=id(2);let desktop=id(3);
  boot.start_init(owner,launch(owner),settings(),11).unwrap();
  boot.start_desktop(desktop,launch(desktop),settings(),22).unwrap();
  assert_eq!(boot.phase(),Phase::DesktopReady);
  boot.session.create_window(desktop,Window{pid:3,generation:6,id:55,
   rect:Rect{x:1,y:1,w:100,h:100}}).unwrap();
  let mut surface=Surfaces::<2>::new();
  surface.submit(&boot.session.windows,desktop,55,
   Rect{x:0,y:0,w:50,h:30}).unwrap();
  assert_eq!(surface.next().unwrap().owner,desktop);
  let mut acl=Access::<2>::new();
  acl.grant(desktop,VIEW|FILE_WRITE|SOCKET,100).unwrap();
  let mut disk=Scratch{data:[0;64],len:0};
  batch6_storage::replace_verified(&acl,desktop,10,"/home/config",
   b"theme=light",&mut disk).unwrap();
  let mut out=[0u8;32];
  assert_eq!(batch6_storage::read_verified(&mut disk,"/home/config",&mut out),
   Ok(11));
  let mut ports=Ports::<2>::new();ports.bind(&acl,desktop,8443,10,30).unwrap();
  assert_eq!(ports.require(desktop,8443,15),Ok(()));
  boot.process_exited(desktop).unwrap();surface.retire(desktop);
  ports.retire(desktop);acl.revoke(desktop).unwrap();
  assert_eq!(boot.phase(),Phase::Degraded);
  assert!(acl.require(desktop,FILE_WRITE,16).is_err());
  assert!(ports.require(desktop,8443,16).is_err());
 }
 #[test]fn physical_driver_and_non_native_app_remain_guarded(){
  let p=Pci{vendor:0x8086,device:1,class:1,subclass:8,
   prog_if:2,mmio:0x1000,mmio_len:4096,irq:16};
  let mut drivers=crate::batch5_devices::IoPolicy::<1>::new();
  assert_eq!(batch6_devices::isolate_storage(&mut drivers,4,2,p),
   Ok(batch6_devices::Kind::Nvme));
  drivers.quarantine(4,2).unwrap();
  assert!(drivers.guard_mmio(4,2,0x1000,4).is_err());
  let mut elf=[0u8;64];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  assert_eq!(batch6_apps::classify(Requested{format:Format::ElfLinux,
   verified:true,bytes:&elf,mac:None},62),Ok(Availability::RuntimeMissing));
 }
 #[test]fn crash_policy_is_finite_and_not_claimed_as_hardware_proof(){
  let mut recovery=Recovery::<2>::new(1);
  let person=id(8);
  assert_eq!(recovery.fault(person,14,0,42),Ok(true));
  assert_eq!(recovery.fault(person,14,0,43),Ok(false));
  recovery.boot_sample(12).unwrap();
  assert!(recovery.require_boot_budget(25).is_ok());
 }
}