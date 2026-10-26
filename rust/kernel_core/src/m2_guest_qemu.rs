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
use crate::m2_blob_store::{BlobStore,BundleSource,Error as BlobError,
 CAPACITY,REQUIRED_SECTORS};
use crate::m2_bundle_manifest::{SignedBundle,BundleManifest,
 stage_verified,Error as BundleError};
use crate::m2_sha256::Stream;
use crate::m2_large_launch;
use core::{ptr,ffi::c_char};
use crate::{ProcessTable,indianos_process_create,indianos_process_success};
const START:u64=24;
const COUNT:u64=100;
const TOTAL:u64=512; // Disposable QEMU-only disk, not user-owned storage.
const BLOB_START:u64=144; // Dedicated second partition, disjoint from /pkg.
const LARGE_V1:usize=32768;
const LARGE_V2:usize=CAPACITY;
const TEST_LARGE_DIGEST_1:[u8;32]=[66,170,89,241,188,5,94,242,150,19,139,86,242,119,252,171,28,229,138,227,42,164,108,132,146,72,213,59,66,125,183,12];
const TEST_LARGE_DIGEST_2:[u8;32]=[229,24,119,15,52,112,4,6,226,151,238,50,108,247,143,127,23,214,162,69,68,169,110,14,57,241,38,126,200,192,192,8];
const TEST_LARGE_SIG_1:[u8;64]=[174,196,218,61,18,135,140,174,79,5,242,209,29,21,112,100,136,131,129,36,37,94,40,191,108,141,87,219,105,88,22,252,68,151,76,138,85,77,62,64,74,209,163,255,9,193,161,106,18,87,4,2,160,186,101,9,197,246,67,213,116,197,118,1];
const TEST_LARGE_SIG_2:[u8;64]=[167,235,41,35,142,158,140,93,154,144,232,64,140,159,123,152,164,253,0,85,36,36,178,181,149,3,152,120,134,208,47,102,0,48,236,93,15,48,219,171,177,43,52,232,192,69,74,235,253,202,44,29,65,130,51,3,229,37,25,18,92,10,30,4];
#[repr(align(4096))]
struct TestLargeScratch([u8;CAPACITY]);
// One CPU, one synchronous test execution. No references persist across
// guest invocations; scratch cleared after every attempted launch.
static mut LARGE_SCRATCH:TestLargeScratch=TestLargeScratch([0;CAPACITY]);
unsafe extern "C"{fn serial_write(message:*const c_char);}
fn large_log(s:&'static[u8]){unsafe{serial_write(s.as_ptr().cast())};}
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

fn large_len(v:u8)->usize{if v==1{LARGE_V1}else{LARGE_V2}}
struct QemuLargeSource{v:u8}
impl BundleSource for QemuLargeSource{
 fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()>{
  let start=offset as usize;
  let end=start.checked_add(out.len()).ok_or(())?;
  let len=large_len(self.v);
  if !(1..=2).contains(&self.v)||end>len{return Err(());}
  // Stream from a deterministic generator; NEVER allocate 64KiB on
  // the small freestanding kernel stack. The old 2.4KiB ELF header,
  // trap probe and exit sequence remain identical.
  let old=elf(self.v);
  let size=(len as u64).to_le_bytes();
  for (i,byte) in out.iter_mut().enumerate(){
   let at=start+i;
   *byte=(at as u8).wrapping_mul(31)^0xa7^self.v;
   if at<147{*byte=old[at];}
   if (96..112).contains(&at){*byte=size[(at-96)%8];}
   if at==len-1{*byte=self.v;}
  }
  Ok(())
 }
}
fn bundle(v:u8)->SignedBundle{
 SignedBundle{
  manifest:BundleManifest{
   app:APP,version:v as u64,length:large_len(v) as u32,
   digest:if v==1{TEST_LARGE_DIGEST_1}else{TEST_LARGE_DIGEST_2},
   rights:0,deps:[0;4],minimums:[0;4],signer:7,epoch:2,
  },
  signature:if v==1{TEST_LARGE_SIG_1}else{TEST_LARGE_SIG_2},
 }
}
fn verified_generator_digest(v:u8)->Result<[u8;32],()>{
 let mut h=Stream::new();
 let mut src=QemuLargeSource{v};
 let mut chunk=[0u8;SECTOR_BYTES];
 for offset in (0..large_len(v)).step_by(SECTOR_BYTES){
  let length=(large_len(v)-offset).min(SECTOR_BYTES);
  src.read_exact(offset as u32,&mut chunk[..length])?;
  h.update(&chunk[..length]).map_err(|_|())?;
 }
 Ok(h.finish())
}
fn run_large_cpu<D:BlockDevice,V:Verify>(
 store:&mut BlobStore<D>,signed:&SignedBundle,verifier:&V
)->Result<(),()>{
 let mut table=ProcessTable::empty();
 let pcb=unsafe{indianos_process_create(&mut table)};
 if pcb.is_null(){return Err(());}
 let id=Identity{pid:unsafe{(*pcb).pid},generation:1};
 let mut approval=crate::m2_consent::Ledger::<1>::new();
 // Exclusively access the test-only static buffer. The code never
 // permits concurrent guest processes or asynchronous borrowing.
 let scratch=unsafe{&mut (*ptr::addr_of_mut!(LARGE_SCRATCH)).0};
 if m2_large_launch::execute_large_approved(store,signed,
   &access(),ADMIN,1,&ROOTS,verifier,unsafe{&mut*pcb},1,
   &approval,scratch)!=Err(m2_large_launch::Error::ConsentDenied){
  return Err(());
 }
 approval.approve(&access(),ADMIN,1,id,
   m2_large_launch::record(signed),0,90).map_err(|_|())?;
 if m2_large_launch::execute_large_approved(store,signed,
   &access(),ADMIN,1,&ROOTS,verifier,unsafe{&mut*pcb},2,
   &approval,scratch)!=Err(m2_large_launch::Error::ConsentDenied){
  return Err(());
 }
 let app=m2_large_launch::execute_large_approved(store,signed,
   &access(),ADMIN,1,&ROOTS,verifier,unsafe{&mut*pcb},1,
   &approval,scratch).map_err(|_|())?;
 approval.retire(id);
 if app.id!=APP||app.version!=signed.manifest.version||
    !unsafe{indianos_process_success(pcb)}||
    crate::m2_syscall_guard::last_denied_calls()!=1||
    scratch.iter().any(|byte|*byte!=0){
  return Err(());
 }
 large_log(b"IndianOS: M2 Phase 5C real 64KiB signed ELF executed in ring3\n\0");
 Ok(())
}
fn run_large<D:BlockDevice,V:Verify>(
 store:&mut BlobStore<D>,stage:u8,verifier:&V
)->Result<(),()>{
 // The embedded manifests are signed by the test publisher key, NOT
 // the untrusted disk's metadata hash. Validate source generation too.
 for v in 1..=2{
  if verified_generator_digest(v)?!=bundle(v).manifest.digest{
   return Err(());
  }
 }
 match stage{
  0=>{
   if store.active().is_some(){return Err(());}
   let mut tampered=bundle(1);
   tampered.signature[0]^=1;
   let original=store.active();
   if !matches!(stage_verified(store,&tampered,&ROOTS,verifier,
       &mut QemuLargeSource{v:1}),Err(BundleError::Signature(_)))||
       store.active()!=original{return Err(());}
   stage_verified(store,&bundle(1),&ROOTS,verifier,
     &mut QemuLargeSource{v:1}).map_err(|_|())?;
   run_large_cpu(store,&bundle(1),verifier)?;
  },
  1=>{
   if store.active().is_none_or(|item|
      item.version!=1||item.length as usize!=LARGE_V1){
    return Err(());
   }
   stage_verified(store,&bundle(2),&ROOTS,verifier,
      &mut QemuLargeSource{v:2}).map_err(|_|())?;
   if store.backup().is_none_or(|item|item.version!=1){
    return Err(());
   }
   run_large_cpu(store,&bundle(2),verifier)?;
  },
  2=>{
   if store.active().is_none_or(|item|
      item.version!=2||item.length as usize!=LARGE_V2){
    return Err(());
   }
   if store.rollback().map_err(|_|())?.version!=1{return Err(());}
   run_large_cpu(store,&bundle(1),verifier)?;
  },
  3=>{
   if store.active().is_none_or(|item|
     item.version!=1||item.length as usize!=LARGE_V1){return Err(());}
   // A once-valid, signed BUT no-longer-current v2 cannot bypass the
   // A/B active version binding. This does not uninstall the package.
   let mut table=ProcessTable::empty();
   let pcb=unsafe{indianos_process_create(&mut table)};
   if pcb.is_null(){return Err(());}
   let mut approval=crate::m2_consent::Ledger::<1>::new();
   let owner=Identity{pid:unsafe{(*pcb).pid},generation:1};
   approval.approve(&access(),ADMIN,1,owner,
      m2_large_launch::record(&bundle(2)),0,90).map_err(|_|())?;
   let scratch=unsafe{&mut (*ptr::addr_of_mut!(LARGE_SCRATCH)).0};
   if m2_large_launch::execute_large_approved(store,&bundle(2),
      &access(),ADMIN,1,&ROOTS,verifier,unsafe{&mut*pcb},1,
      &approval,scratch)!=Err(m2_large_launch::Error::StaleBundle){
    return Err(());
   }
   run_large_cpu(store,&bundle(1),verifier)?;
  },
  _=>return Err(())
 }
 large_log(b"IndianOS: M2 Phase 5C dedicated A/B blob partition verified\n\0");
 Ok(())
}

pub fn run()->Result<u8,()>{
 let mut disk=unsafe{AtaPio::probe()}.map_err(|_|())?;
 if disk.sector_count()!=TOTAL{return Err(());}
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
 // A second, separate, explicitly reserved 258-sector partition is
 // used for larger packages; the first /pkg filesystem cannot see it.
 {
  let window=BlockWindow::new(&mut disk,BLOB_START,REQUIRED_SECTORS)
   .map_err(|_|())?;
  let mut large=BlobStore::mount(window).map_err(|_|())?;
  run_large(&mut large,stage,&verifier)?;
 }
 // Byte-for-byte untouched disk canaries beyond BOTH package windows.
 // The only writeable regions are [24,124) and [144,402).
 for lba in (0..START)
   .chain(START+COUNT..BLOB_START)
   .chain(BLOB_START+REQUIRED_SECTORS..TOTAL){
  let mut sector=[0u8;SECTOR_BYTES];
  disk.read_sector(lba,&mut sector).map_err(|_|())?;
  if sector.iter().any(|&x|x!=0){return Err(());}
 }
 Ok(stage+1)
}
