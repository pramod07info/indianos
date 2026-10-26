//! M2 Phase 4C QEMU-only four-boot acceptance with REAL Ed25519.
//! Authenticated test boot policy, signed M2P6 ELF and actual CPU ring3.
//! Test-only publisher/boot public keys are NEVER production trust roots.
use crate::ata_pio::AtaPio;
use crate::block::{BlockDevice,BlockWindow,SECTOR_BYTES};
use crate::hier_vfs::HierVfs;
use crate::durablefs::FsError;
use crate::m2_inventory::{Inventory,Error as PackageError,Record,MAX_IMAGE_BYTES};
use crate::m2_sdk::{Package,ABI};
use crate::batch4_signature::{Envelope,Root,Verify};
use crate::m2_ed25519::Ed25519Backend;
use crate::m2_trust::{Anchor,Trusted};
use crate::m2_boot_trust::{BootTrust,HardwareTrust,PolicySlots,
 Error as TrustBootError};
use crate::m2_trust_policy::SNAPSHOT_BYTES;
use crate::m2_trust_policy::SignedSnapshot;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};
use crate::{ProcessTable,indianos_process_create,indianos_process_success};
const START:u64=24;
const COUNT:u64=100;
const APP:u32=9;
// Deliberately larger than the old 1870-byte M2P3 maximum.
const IMAGE_LEN:usize=2400;
const ADMIN:Identity=Identity{pid:44,generation:1};
const ROOTS:[Root;1]=[Root{signer:7,epoch:2,enabled:true}];
// These public test vectors and signatures are compiled only for disposable
// QEMU acceptance. No private signing key is embedded anywhere.
const TEST_PUBLISHER:[u8;32]=[72,7,90,89,126,114,26,21,110,46,7,153,222,92,192,197,50,77,198,231,234,241,205,212,98,80,134,142,197,50,21,221];
const TEST_BOOT_ROOT:[u8;32]=[32,18,203,144,202,96,232,229,216,218,246,110,34,114,210,35,62,4,134,213,87,232,198,97,65,237,137,32,23,125,126,183];
const TEST_BOOT_SIG:[u8;64]=[119,72,45,124,7,155,22,101,144,76,124,204,209,87,120,49,147,230,213,149,252,255,79,198,234,73,239,233,97,36,149,99,34,40,70,94,244,213,101,150,27,204,8,204,212,144,228,34,26,95,59,112,156,232,160,211,65,178,125,176,175,60,242,11];
const TEST_APP_SIG_1:[u8;64]=[63,81,43,0,190,178,223,51,88,189,167,181,91,103,50,139,190,90,11,13,224,164,90,78,55,131,33,51,12,27,158,212,136,7,135,17,212,55,183,170,27,95,27,70,70,158,14,123,37,245,126,11,234,8,37,236,34,83,247,131,173,57,39,5];
const TEST_APP_SIG_2:[u8;64]=[21,236,129,187,72,77,222,161,99,201,245,2,139,214,135,125,24,110,124,254,153,118,44,22,17,145,129,61,228,174,80,94,86,142,37,227,47,112,113,194,96,221,32,250,34,169,175,160,229,72,132,162,249,106,31,196,76,46,231,53,103,225,171,13];
/// QEMU ONLY: mimics two hardware trust operations with plain RAM.
/// This is NOT a production UEFI Secure Boot or TPM implementation.
#[derive(Clone,Copy)]
struct QemuFakePlatform{floor:u64,root:[u8;32],fails_advance:bool}
impl HardwareTrust for QemuFakePlatform{
 fn read_authenticated_boot_root(&self)->Result<[u8;32],()>{
  Ok(self.root) // QEMU test fixture, NOT authenticated physical firmware
 }
 fn read_protected_sequence(&self)->Result<u64,()>{Ok(self.floor)}
 fn advance_protected_sequence(&mut self,seq:u64)->Result<(),()>{
  if self.fails_advance||seq<self.floor{return Err(());}
  self.floor=seq;Ok(())
 }
}
#[derive(Clone,Copy)]
struct QemuFakePolicySlots{data:[Option<[u8;SNAPSHOT_BYTES]>;2]}
impl PolicySlots for QemuFakePolicySlots{
 fn read(&mut self,index:usize,out:&mut[u8;SNAPSHOT_BYTES])
   ->Result<bool,()>{
  if let Some(bytes)=self.data[index]{*out=bytes;Ok(true)}
  else{Ok(false)}
 }
 fn replace_durably(&mut self,index:usize,b:&[u8;SNAPSHOT_BYTES])
   ->Result<(),()>{
  self.data[index]=Some(*b);Ok(())
 }
}
type TestBoot=BootTrust<QemuFakePlatform,QemuFakePolicySlots>;
fn test_signed_policy()->SignedSnapshot{
 let mut policy=SignedSnapshot::empty();
 policy.sequence=3;
 policy.count=1;
 policy.publishers[0]=Anchor{signer:7,epoch:2,
  public_key:TEST_PUBLISHER,enabled:true};
 policy.signature=TEST_BOOT_SIG;
 policy
}
fn trusted_boot_session()->Result<TestBoot,()>{
 let snapshot=test_signed_policy();
 let memory=QemuFakePolicySlots{data:[Some(snapshot.to_bytes()),None]};
 let hardware=QemuFakePlatform{floor:0,root:TEST_BOOT_ROOT,
     fails_advance:false};
 let mut boot=BootTrust::new(hardware,memory);
 if boot.registry().is_some(){return Err(());}
 if boot.boot_recover(&access(),ADMIN,1)!=Ok(3){return Err(());}
 if boot.platform().floor!=3||
  boot.registry().ok_or(())?.public_key(7,2)!=Ok(TEST_PUBLISHER){
  return Err(());
 }
 // A one-bit-changed but structurally valid Ed25519 signature CANNOT
 // introduce a new boot root, even with a clean untrusted disk.
 let mut corrupt=snapshot;
 corrupt.signature[0]^=1;
 let bad=QemuFakePolicySlots{data:[Some(corrupt.to_bytes()),None]};
 let mut rejected=BootTrust::new(hardware,bad);
 if rejected.boot_recover(&access(),ADMIN,1)!=Err(TrustBootError::NoValidPolicy)||
   rejected.registry().is_some(){return Err(());}
 // A perfectly valid OLD signed policy is insufficient when the
 // protected monotonic counter is newer. This floor is SIMULATED.
 let mut replay=BootTrust::new(
  QemuFakePlatform{floor:4,..hardware},memory);
 if replay.boot_recover(&access(),ADMIN,1)!=Err(TrustBootError::NoValidPolicy)||
   replay.registry().is_some(){return Err(());}
 let mut unavailable=BootTrust::new(
  QemuFakePlatform{fails_advance:true,..hardware},memory);
 if unavailable.boot_recover(&access(),ADMIN,1)
    !=Err(TrustBootError::PlatformUnavailable)||
   unavailable.registry().is_some(){return Err(());}
 Ok(boot)
}
fn elf(v:u8)->[u8;IMAGE_LEN]{
 let mut b=[0u8;IMAGE_LEN];
 b[..4].copy_from_slice(b"\x7fELF");
 b[4]=2;b[5]=1;b[6]=1;
 b[16..18].copy_from_slice(&2u16.to_le_bytes());
 b[18..20].copy_from_slice(&62u16.to_le_bytes());
 b[20..24].copy_from_slice(&1u32.to_le_bytes());
 b[24..32].copy_from_slice(&0x400078u64.to_le_bytes());
 b[32..40].copy_from_slice(&64u64.to_le_bytes());
 b[52..54].copy_from_slice(&64u16.to_le_bytes());
 b[54..56].copy_from_slice(&56u16.to_le_bytes());
 b[56..58].copy_from_slice(&1u16.to_le_bytes());
 b[64..68].copy_from_slice(&1u32.to_le_bytes());
 b[68..72].copy_from_slice(&5u32.to_le_bytes()); // R+X; never W+X
 b[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
 b[96..104].copy_from_slice(&(IMAGE_LEN as u64).to_le_bytes());
 b[104..112].copy_from_slice(&(IMAGE_LEN as u64).to_le_bytes());
 b[112..120].copy_from_slice(&4096u64.to_le_bytes());
 // mov eax,2 ; xor edi,edi ; xor esi,esi ; int80 ; hlt ; nop,nop,nop
 // A forbidden syscall 7 must return BAD (-1) from the REAL trap gate.
 // Otherwise branch to UD2; only the successful rejection may exit(0).
 b[120..147].copy_from_slice(&[
  0xB8,7,0,0,0,0x31,0xFF,0x31,0xF6,0xCD,0x80,
  0x48,0x83,0xF8,0xFF,0x75,0x08,
  0xB8,2,0,0,0,0xCD,0x80,0xF4,0x0F,0x0B
 ]);
 b[IMAGE_LEN-1]=v;
 b
}
fn signed_record(v:u8)->Record{
 let p=package(v);
 Record{id:p.id,version:p.version,rights:p.rights,digest:p.payload_digest,
  deps:p.deps,minimums:[0;4]}
}
fn envelope(v:u8)->Envelope{
 Envelope{signer:7,epoch:2,
  digest:crate::m2_manifest::digest(signed_record(v),7,2),
  signature:if v==1{TEST_APP_SIG_1}else{TEST_APP_SIG_2}}
}
fn package(v:u8)->Package{
 Package{id:APP,version:v as u64,abi:ABI,rights:0,deps:[0;4],payload_digest:crate::m2_sha256::digest(&elf(v))}
}
fn access()->Access<1>{
 let mut a=Access::new();a.grant(ADMIN,INSTALL,100).unwrap();a
}
fn check<D:BlockDevice,V:Verify>(inv:&mut Inventory<D>,v:u8,verifier:&V)->Result<(),()>{
 let mut b=[0u8;MAX_IMAGE_BYTES];
 let (r,n)=inv.verified_image(&access(),ADMIN,1,&ROOTS,verifier,
     APP,&mut b).map_err(|_|())?;
 if r.version!=v as u64||n!=IMAGE_LEN||b[..n]!=elf(v){return Err(());}
 Ok(())
}
fn run_cpu<D:BlockDevice,V:Verify>(inv:&mut Inventory<D>,verifier:&V)->Result<(),()>{
 let mut table=ProcessTable::empty();
 let pcb=unsafe{indianos_process_create(&mut table)};
 if pcb.is_null(){return Err(());}
 // Revoke signer root and prove trusted storage refuses this exact image.
 let disabled=[Root{signer:7,epoch:2,enabled:false}];
 let mut scratch=[0u8;MAX_IMAGE_BYTES];
 if inv.verified_image(&access(),ADMIN,1,&disabled,verifier,
       APP,&mut scratch)!=Err(PackageError::Trust(
         crate::batch4_signature::Error::Untrusted)){return Err(());}
 let saved=inv.installed(APP).map_err(|_|())?.ok_or(())?;
 let process=Identity{pid:unsafe{(*pcb).pid},generation:1};
 let mut consent=crate::m2_consent::Ledger::<1>::new();
 // Refuse unapproved execution before a single ring-3 instruction.
 if crate::m2_launch::execute_installed_approved(inv,&access(),ADMIN,1,
   &ROOTS,verifier,APP,unsafe{&mut*pcb},1,&consent)
     !=Err(crate::m2_launch::Error::ConsentDenied){return Err(());}
 consent.approve(&access(),ADMIN,1,process,saved,saved.rights,90)
   .map_err(|_|())?;
 if crate::m2_launch::execute_installed_approved(
  inv,&access(),ADMIN,1,&ROOTS,verifier,APP,
  unsafe{&mut*pcb},2,&consent)
    !=Err(crate::m2_launch::Error::ConsentDenied){return Err(());}
 let receipt=crate::m2_launch::execute_installed_approved(
  inv,&access(),ADMIN,1,&ROOTS,verifier,APP,
  unsafe{&mut*pcb},process.generation,&consent).map_err(|_|())?;
 consent.retire(process);
 if receipt.id!=APP||!unsafe{indianos_process_success(pcb)}{return Err(());}
 if crate::m2_syscall_guard::last_denied_calls()!=1{return Err(());}
 Ok(())
}
pub fn run()->Result<u8,()>{
 let mut disk=unsafe{AtaPio::probe()}.map_err(|_|())?;
 if disk.sector_count()!=128{return Err(());}
 let mut left=[0u8;SECTOR_BYTES];let mut right=[0u8;SECTOR_BYTES];
 disk.read_sector(START,&mut left).map_err(|_|())?;
 disk.read_sector(START+1,&mut right).map_err(|_|())?;
 let blank=left.iter().all(|&x|x==0)&&right.iter().all(|&x|x==0);
 let slice=BlockWindow::new(&mut disk,START,COUNT).map_err(|_|())?;
 let mut fs=if blank{HierVfs::format_blank(slice).map_err(|_|())?}
     else{HierVfs::mount(slice).map_err(|_|())?};
 let mut byte=[0u8;1];
 let stage=match fs.read("/phase",&mut byte){
  Ok(1) if !blank=>byte[0],
  Err(FsError::NotFound) if blank=>{fs.create("/phase",&[0]).map_err(|_|())?;0},
  _=>return Err(())
 };
 if stage>3{return Err(());}
 let mut inv=Inventory::from_mounted(fs);
 inv.init_directory().map_err(|_|())?;
 // Each reboot verifies a signed boot policy with real Ed25519 BEFORE
 // trusting any publisher key from it.
 let trusted=trusted_boot_session()?;
 let backend=Ed25519Backend;
 let verifier=Trusted{registry:trusted.registry().ok_or(())?,backend:&backend};
 match stage{
  0=>{
   if inv.installed(APP).map_err(|_|())?.is_some(){return Err(());}
   let mut invalid=envelope(1);
   invalid.signature[0]^=1;
   if inv.install_manifest_image(&access(),ADMIN,1,&ROOTS,&verifier,
      &invalid,&elf(1),package(1))!=Err(PackageError::Trust(
        crate::batch4_signature::Error::InvalidSignature)){return Err(());}
   if inv.installed(APP).map_err(|_|())?.is_some(){return Err(());}
   inv.install_manifest_image(&access(),ADMIN,1,&ROOTS,&verifier,
       &envelope(1),&elf(1),package(1)).map_err(|_|())?;
   check(&mut inv,1,&verifier)?;run_cpu(&mut inv,&verifier)?;
  },
  1=>{
   check(&mut inv,1,&verifier)?;
   inv.install_manifest_image(&access(),ADMIN,1,&ROOTS,&verifier,
       &envelope(2),&elf(2),package(2)).map_err(|_|())?;
   check(&mut inv,2,&verifier)?;run_cpu(&mut inv,&verifier)?;
  },
  2=>{
   check(&mut inv,2,&verifier)?;
   let prior=inv.rollback(&access(),ADMIN,1,APP).map_err(|_|())?;
   if prior.version!=1{return Err(());}
   check(&mut inv,1,&verifier)?;run_cpu(&mut inv,&verifier)?;
  },
  3=>{
   check(&mut inv,1,&verifier)?;
   inv.remove(&access(),ADMIN,1,APP).map_err(|_|())?;
   if inv.installed(APP).map_err(|_|())?.is_some(){return Err(());}
   let mut buf=[0u8;MAX_IMAGE_BYTES];
   if inv.verified_image(&access(),ADMIN,1,&ROOTS,&verifier,
       APP,&mut buf)!=Err(PackageError::NotFound){return Err(());}
   let mut table=ProcessTable::empty();
   let pcb=unsafe{indianos_process_create(&mut table)};
   if pcb.is_null()||crate::m2_launch::execute_installed_approved(
       &mut inv,&access(),ADMIN,1,&ROOTS,&verifier,APP,
       unsafe{&mut*pcb},1,&crate::m2_consent::Ledger::<1>::new())!=Err(crate::m2_launch::Error::Stored(PackageError::NotFound)){
       return Err(());
   }
  },
  _=>return Err(())
 }
 let device=inv.into_device();
 let mut volume=HierVfs::mount(device).map_err(|_|())?;
 volume.overwrite("/phase",&[stage+1]).map_err(|_|())?;
 drop(volume);
 // Check every byte outside the bounded 100-sector partition.
 // The installer never needs or accesses a network connection.
 for lba in (0..START).chain(START+COUNT..128){
  let mut sector=[0u8;SECTOR_BYTES];
  disk.read_sector(lba,&mut sector).map_err(|_|())?;
  if sector.iter().any(|&x|x!=0){return Err(());}
 }
 Ok(stage+1)
}
