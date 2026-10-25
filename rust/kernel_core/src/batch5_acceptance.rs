//! Batch 5 integration acceptance. Host-side test DOES NOT certify real
//! hardware, actual userspace desktop, crypto backends or app compatibility.
#[cfg(test)] mod tests{
 use crate::batch5_session::{Session,Identity,SessionError};
 use crate::batch5_vfs::{FileTable,GuestVfs};
 use crate::batch5_installer::{Installer,PackageStore};
 use crate::batch5_devices::IoPolicy;
 use crate::batch5_network::{Client,parse_ack};
 use crate::batch5_compat::{self,Route};
 use crate::batch5_release::qualify_release;
 use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 use crate::batch2_windows::{Window,Rect};
 use crate::batch3_ahci::Region;
 use crate::batch4_signature::{Verify,Envelope,Root};
 use crate::batch4_release::{Evidence as ReleaseEvidence,Status};
 use crate::batch3_hardware_matrix::{Evidence as HardwareEvidence,Platform,REQUIRED};
 use crate::batch4_performance::{Monitor,Stage};
 struct FakeFs;
 impl GuestVfs for FakeFs{
  fn read_at(&mut self,path:&[u8],_offset:u64,out:&mut[u8])->Result<usize,()>{
   if path!=b"/docs/status"{return Err(());}
   let bytes=b"IndianOS";let size=bytes.len().min(out.len());
   out[..size].copy_from_slice(&bytes[..size]);Ok(size)
  }
 }
 struct Verifier;
 impl Verify for Verifier{
  fn verify_digest_and_signature(&self,_:&Envelope,payload:&[u8])->bool{
   payload==b"verified image"
  }
 }
 struct Disk{active:u32}
 impl PackageStore for Disk{
  fn stage_and_flush(&mut self,_:u32,_:&[u8])->Result<(),()>{Ok(())}
  fn activate_and_flush(&mut self,app:u32)->Result<(),()>{self.active=app;Ok(())}
  fn abort_staging(&mut self,_:u32){}
 }
 #[test]fn verified_image_to_revoked_desktop_session_and_atomic_file_reads(){
  let id=Identity{pid:40,generation:7};
  let mut runtime=Session::<2,2,1,2>::new();
  runtime.stage(Launch{pid:40,generation:7,image:Image{
   image_id:9,entry:0x1000,mapped_end:0x2000,verified:true}},
   Settings{theme:Theme::Light,pointer_speed:5,key_repeat:10}).unwrap();
  runtime.consume_launch(id).unwrap();
  runtime.create_window(id,Window{pid:40,generation:7,id:55,
   rect:Rect{x:5,y:5,w:180,h:100}}).unwrap();
  runtime.focus(id,55).unwrap();
  runtime.physical_key(30,true).unwrap();
  assert_eq!(runtime.receive_key(id).unwrap().scan_code,30);
  let mut fds=FileTable::<2>::new();let handle=fds.open(40,7,"/docs/status").unwrap();
  let mut bytes=[0u8;12];assert_eq!(
   fds.read(40,7,handle,0,&mut bytes,&mut FakeFs),Ok(8));
  assert_eq!(&bytes[..8],b"IndianOS");
  fds.retire(id.pid,id.generation);runtime.retire(id).unwrap();
  assert_eq!(runtime.receive(id),Err(SessionError::Unauthorized));
  assert!(fds.read(40,7,handle,0,&mut bytes,&mut FakeFs).is_err());
 }
 #[test]fn crypto_and_consent_both_gate_install(){
  let mut installer=Installer::new(4);
  let envelope=Envelope{signer:4,epoch:1,digest:[2;32],signature:[3;64]};
  let roots=[Root{signer:4,epoch:1,enabled:true}];
  installer.request(&roots,&Verifier,&envelope,b"verified image",9,5,2,1).unwrap();
  let mut disk=Disk{active:4};
  assert!(installer.install(&mut disk,&roots,&Verifier,&envelope,b"verified image",5,2).is_err());
  installer.approve(5,2,[2;32],true,1).unwrap();
  installer.install(&mut disk,&roots,&Verifier,&envelope,b"verified image",5,2).unwrap();
  assert_eq!(disk.active,9);installer.boot_health(true).unwrap();
 }
 #[test]fn authenticated_driver_lease_is_not_dma_bypass(){
  let mut io=IoPolicy::<1>::new();
  io.grant(1,2,0x1000,4096,7).unwrap();
  assert_eq!(io.ahci_preflight(1,2,0x1000,1,8,100,
   &[Region{dma_address:0x4000,byte_count:4096}]),Ok(4096));
  io.quarantine(1,2).unwrap();
  assert!(io.guard_mmio(1,2,0x1000,4).is_err());
 }
 #[test]fn third_party_formats_are_not_declared_native(){
  let mut elf=[0u8;64];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  assert_eq!(batch5_compat::linux_x86_64_elf(true,&elf),
   Ok(Route::NeedsLinuxRuntime));
  assert!(batch5_compat::windows_x86_64(false,b"MZ").is_err());
 }
 #[test]fn release_does_not_infer_evidence_from_unit_tests(){
  let sha=[7;20];
  let hardware=HardwareEvidence{platform:Platform::QemuX86_64,
   commit:sha,passed:REQUIRED,failed:0,skipped:0};
  let policy=ReleaseEvidence{sha,kernel:Status::Pass,filesystem:Status::Pass,
   security:Status::Pass,update:Status::Pass,compatibility:Status::Pass,
   platform_mask:1,independently_reviewed:false};
  let mut metrics=Monitor::new();metrics.record(Stage::Boot,5).unwrap();
  assert!(qualify_release(sha,hardware,Platform::QemuX86_64,
   policy,&metrics,10).is_err());
 }
}