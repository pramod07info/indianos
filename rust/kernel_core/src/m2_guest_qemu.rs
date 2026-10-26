//! M2 QEMU-only four-boot acceptance: installed ELF bytes execute in real
//! ring3 CPU mode from the CRC-backed M2P3 filesystem. NOT production crypto.
//! This feature can only be enabled explicitly on disposable QEMU 128-sector IDE.
use crate::ata_pio::AtaPio;
use crate::block::{BlockDevice,BlockWindow,SECTOR_BYTES};
use crate::hier_vfs::HierVfs;
use crate::durablefs::FsError;
use crate::m2_inventory::{Inventory,Error as PackageError,Record,MAX_IMAGE_BYTES};
use crate::m2_sdk::{Package,ABI};
use crate::batch4_signature::{Envelope,Root,Verify};
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
  signature:[40+v;64]}
}
fn package(v:u8)->Package{
 Package{id:APP,version:v as u64,abi:ABI,rights:0,deps:[0;4],payload_digest:crate::m2_sha256::digest(&elf(v))}
}
fn access()->Access<1>{
 let mut a=Access::new();a.grant(ADMIN,INSTALL,100).unwrap();a
}
struct FixtureVerifier;
impl Verify for FixtureVerifier{
 fn verify_digest_and_signature(&self,e:&Envelope,data:&[u8])->bool{
  // TEST-ONLY PINNED BYTES. This does NOT implement Ed25519 or hash signing.
  (1u8..=2).any(|v|e.signer==7&&e.epoch==2&&
    e.signature==[40+v;64]&&e.digest==
      crate::m2_manifest::digest(signed_record(v),7,2)&&
    data==crate::m2_manifest::canonical(signed_record(v),7,2))
 }
}
fn check<D:BlockDevice>(inv:&mut Inventory<D>,v:u8)->Result<(),()>{
 let mut b=[0u8;MAX_IMAGE_BYTES];
 let (r,n)=inv.verified_image(&access(),ADMIN,1,&ROOTS,&FixtureVerifier,
     APP,&mut b).map_err(|_|())?;
 if r.version!=v as u64||n!=IMAGE_LEN||b[..n]!=elf(v){return Err(());}
 Ok(())
}
fn run_cpu<D:BlockDevice>(inv:&mut Inventory<D>)->Result<(),()>{
 let mut table=ProcessTable::empty();
 let pcb=unsafe{indianos_process_create(&mut table)};
 if pcb.is_null(){return Err(());}
 // Revoke signer root and prove trusted storage refuses this exact image.
 let disabled=[Root{signer:7,epoch:2,enabled:false}];
 let mut scratch=[0u8;MAX_IMAGE_BYTES];
 if inv.verified_image(&access(),ADMIN,1,&disabled,&FixtureVerifier,
       APP,&mut scratch)!=Err(PackageError::Trust(
         crate::batch4_signature::Error::Untrusted)){return Err(());}
 let saved=inv.installed(APP).map_err(|_|())?.ok_or(())?;
 let process=Identity{pid:unsafe{(*pcb).pid},generation:1};
 let mut consent=crate::m2_consent::Ledger::<1>::new();
 // Refuse unapproved execution before a single ring-3 instruction.
 if crate::m2_launch::execute_installed_approved(inv,&access(),ADMIN,1,
   &ROOTS,&FixtureVerifier,APP,unsafe{&mut*pcb},1,&consent)
     !=Err(crate::m2_launch::Error::ConsentDenied){return Err(());}
 consent.approve(&access(),ADMIN,1,process,saved,saved.rights,90)
   .map_err(|_|())?;
 if crate::m2_launch::execute_installed_approved(
  inv,&access(),ADMIN,1,&ROOTS,&FixtureVerifier,APP,
  unsafe{&mut*pcb},2,&consent)
    !=Err(crate::m2_launch::Error::ConsentDenied){return Err(());}
 let receipt=crate::m2_launch::execute_installed_approved(
  inv,&access(),ADMIN,1,&ROOTS,&FixtureVerifier,APP,
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
 match stage{
  0=>{
   if inv.installed(APP).map_err(|_|())?.is_some(){return Err(());}
   inv.install_manifest_image(&access(),ADMIN,1,&ROOTS,&FixtureVerifier,
       &envelope(1),&elf(1),package(1)).map_err(|_|())?;
   check(&mut inv,1)?;run_cpu(&mut inv)?;
  },
  1=>{
   check(&mut inv,1)?;
   inv.install_manifest_image(&access(),ADMIN,1,&ROOTS,&FixtureVerifier,
       &envelope(2),&elf(2),package(2)).map_err(|_|())?;
   check(&mut inv,2)?;run_cpu(&mut inv)?;
  },
  2=>{
   check(&mut inv,2)?;
   let prior=inv.rollback(&access(),ADMIN,1,APP).map_err(|_|())?;
   if prior.version!=1{return Err(());}
   check(&mut inv,1)?;run_cpu(&mut inv)?;
  },
  3=>{
   check(&mut inv,1)?;
   inv.remove(&access(),ADMIN,1,APP).map_err(|_|())?;
   if inv.installed(APP).map_err(|_|())?.is_some(){return Err(());}
   let mut buf=[0u8;MAX_IMAGE_BYTES];
   if inv.verified_image(&access(),ADMIN,1,&ROOTS,&FixtureVerifier,
       APP,&mut buf)!=Err(PackageError::NotFound){return Err(());}
   let mut table=ProcessTable::empty();
   let pcb=unsafe{indianos_process_create(&mut table)};
   if pcb.is_null()||crate::m2_launch::execute_installed_approved(
       &mut inv,&access(),ADMIN,1,&ROOTS,&FixtureVerifier,APP,
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
