//! M2 on-device persisted INSTALL INVENTORY with cryptographic verifier
//! dependency, explicit PID-generation capability and one-level rollback.
//! IMPORTANT: the record is not an installed executable: binary ingestion,
//! live ELF loading, trust-key provisioning and App Store UI are not here.
//! Verifier implementation must be a real trusted crypto provider in guest.
use crate::block::BlockDevice;
use crate::hier_vfs::HierVfs;
use crate::durablefs::{FsError,MAX_FILE_BYTES};
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};
use crate::batch4_signature::{self,Envelope,Root,Verify};
use crate::m2_sdk::{Package,Catalog,MAX_DEPS};
const LEN:usize=100;
const LEGACY_LEN:usize=52;
const IMAGE_HEADER:usize=178;
/// Current R027 VFS file bank permits at most 1870 signed image bytes.
pub const MAX_IMAGE_BYTES:usize=MAX_FILE_BYTES-IMAGE_HEADER;
/// Earlier records had no dependency provenance. Never assume they had none.
const UNKNOWN_DEP:u32=u32::MAX;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{
 pub id:u32,pub version:u64,pub rights:u32,pub digest:[u8;32],
 pub deps:[u32;MAX_DEPS],pub minimums:[u64;MAX_DEPS],
}
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Denied,Invalid,NotFound,Downgrade,RollbackPending,MissingDependency,
 InUse,ImageMissing,DigestMismatch,Trust(batch4_signature::Error),Fs(FsError)}
/// Must be implemented by a trusted kernel process-supervisor/ELF loader.
 /// Passing a simple caller-controlled bool is NOT proof of healthy execution.
pub trait RuntimeHealth{
 fn healthy_running_image(&self,app:u32,version:u64,digest:&[u8;32])->bool;
}
pub struct Inventory<D:BlockDevice>{volume:HierVfs<D>}
/// Stable versioned metadata for dependency-safe guest package lifecycle.
/// Convert unknown legacy dependencies to a sentinel rather than erasing them.
fn encode(p:Record)->[u8;LEN]{
 let mut b=[0u8;LEN];b[..4].copy_from_slice(b"M2P2");
 b[4..8].copy_from_slice(&p.id.to_le_bytes());
 b[8..16].copy_from_slice(&p.version.to_le_bytes());
 b[16..20].copy_from_slice(&p.rights.to_le_bytes());
 b[20..52].copy_from_slice(&p.digest);
 for n in 0..MAX_DEPS{
  b[52+n*4..56+n*4].copy_from_slice(&p.deps[n].to_le_bytes());
  b[68+n*8..76+n*8].copy_from_slice(&p.minimums[n].to_le_bytes());
 }
 b
}
fn decode(b:&[u8])->Result<Record,Error>{
 let legacy=b.len()==LEGACY_LEN&&&b[..4]==b"M2PK";
 let current=b.len()==LEN&&&b[..4]==b"M2P2";
 let image=b.len()>=IMAGE_HEADER&&&b[..4]==b"M2P3";
 if !legacy&&!current&&!image{return Err(Error::Invalid);}
 if image{
  let size=u16::from_le_bytes([b[176],b[177]]) as usize;
  let signer=u32::from_le_bytes(b[100..104].try_into().unwrap());
  let epoch=u64::from_le_bytes(b[104..112].try_into().unwrap());
  if size==0||IMAGE_HEADER.checked_add(size)!=Some(b.len())||
     signer==0||epoch==0||b[112..176].iter().all(|&x|x==0){
   return Err(Error::Invalid);
  }
 }
 let id=u32::from_le_bytes(b[4..8].try_into().unwrap());
 let version=u64::from_le_bytes(b[8..16].try_into().unwrap());
 let rights=u32::from_le_bytes(b[16..20].try_into().unwrap());
 let mut digest=[0u8;32];digest.copy_from_slice(&b[20..52]);
 if id==0||id>99||version==0||digest==[0;32]||
    rights&!crate::batch6_security::ALL!=0{return Err(Error::Invalid);}
 let mut deps=[0u32;MAX_DEPS];
 let mut minimums=[0u64;MAX_DEPS];
 if legacy{
  deps=[UNKNOWN_DEP;MAX_DEPS];
 }else{
  for n in 0..MAX_DEPS{
   deps[n]=u32::from_le_bytes(b[52+n*4..56+n*4].try_into().unwrap());
   minimums[n]=u64::from_le_bytes(b[68+n*8..76+n*8].try_into().unwrap());
  }
  if deps!=[UNKNOWN_DEP;MAX_DEPS]||minimums!=[0;MAX_DEPS]{
   for n in 0..MAX_DEPS{
    let dep=deps[n];
    if (dep==0&&minimums[n]!=0)||
       (dep!=0&&(dep>99||dep==id||minimums[n]==0))||
       (dep!=0&&deps[..n].contains(&dep)){return Err(Error::Invalid);}
   }
  }
 }
 Ok(Record{id,version,rights,digest,deps,minimums})
}
fn name(id:u32,old:bool)->Result<[u8;8],Error>{
 if id==0||id>99{return Err(Error::Invalid);}
 let mut p=*b"/pkg/p00";
 if old{p[5]=b'o';}
 p[6]=b'0'+(id/10) as u8;p[7]=b'0'+(id%10) as u8;
 // Exact 8-byte ASCII path; no NUL byte reaches the VFS.
 Ok(p)
}
fn path(p:&[u8;8])->&str{core::str::from_utf8(&p[..8]).unwrap()}
impl<D:BlockDevice> Inventory<D>{
 pub fn from_mounted(volume:HierVfs<D>)->Self{Self{volume}}
 pub fn mount(disk:D)->Result<Self,Error>{
  Ok(Self::from_mounted(HierVfs::mount(disk).map_err(Error::Fs)?))
 }
 pub fn into_device(self)->D{self.volume.into_device()}
 /// Explicit opt-in namespace initialization on already mounted volume.
 pub fn init_directory(&mut self)->Result<(),Error>{
  match self.volume.mkdir("/pkg"){
   Ok(())|Err(FsError::Exists)=>Ok(()),
   Err(e)=>Err(Error::Fs(e)),
  }
 }
 fn read_raw(&mut self,id:u32,old:bool,buf:&mut[u8;MAX_FILE_BYTES])
  ->Result<Option<(Record,usize)>,Error>{
  let p=name(id,old)?;
  match self.volume.read(path(&p),buf){
   Ok(n)=>{
    let rec=decode(&buf[..n])?;
    if rec.id!=id{return Err(Error::Invalid);}
    Ok(Some((rec,n)))
   },
   Err(FsError::NotFound)=>Ok(None),
   Err(e)=>Err(Error::Fs(e)),
  }
 }
 fn read(&mut self,id:u32,old:bool)->Result<Option<Record>,Error>{
  let mut bytes=[0u8;MAX_FILE_BYTES];
  Ok(self.read_raw(id,old,&mut bytes)?.map(|x|x.0))
 }
 pub fn installed(&mut self,id:u32)->Result<Option<Record>,Error>{self.read(id,false)}
 /// Read-only checkpoint status for trusted App Store previews.
 pub fn rollback_pending(&mut self,id:u32)->Result<bool,Error>{
  Ok(self.read(id,true)?.is_some())
 }
 /// Bounded recursive graph check; fail closed on cycles, legacy unknown
 /// edges, missing installed packages or version-regressed prerequisites.
 fn require_runtime_dependencies(&mut self,root:Record)->Result<(),Error>{
  let mut color=[0u8;100];
  self.walk_runtime_dependencies(root,&mut color,0)
 }
 fn walk_runtime_dependencies(&mut self,record:Record,color:&mut[u8;100],
  depth:usize)->Result<(),Error>{
  if record.id==0||record.id>99||depth>16||
    record.deps==[UNKNOWN_DEP;MAX_DEPS]{return Err(Error::MissingDependency);}
  let id=record.id as usize;
  if color[id]==1{return Err(Error::MissingDependency);}
  if color[id]==2{return Ok(());}
  color[id]=1;
  for n in 0..MAX_DEPS{
   let dep=record.deps[n];
   if dep==0{continue;}
   let saved=self.read(dep,false)?.ok_or(Error::MissingDependency)?;
   if saved.version<record.minimums[n]{return Err(Error::MissingDependency);}
   self.walk_runtime_dependencies(saved,color,depth+1)?;
  }
  color[id]=2;Ok(())
 }
 /// Re-validate the saved signature and image *after* remount before copying
 /// bytes to a privileged native loader. INSTALL is deliberately required:
 /// this API is not an unprivileged application binary read syscall.
 /// A trusted production Verify implementation must perform real Ed25519
 /// plus image hashing, not return a caller-controlled approval bit.
 pub fn verified_image<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  id:u32,out:&mut[u8])->Result<(Record,usize),Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (record,n)=self.read_raw(id,false,&mut raw)?.ok_or(Error::NotFound)?;
  if n<IMAGE_HEADER||&raw[..4]!=b"M2P3"{return Err(Error::ImageMissing);}
  let signer=u32::from_le_bytes(raw[100..104].try_into().unwrap());
  let epoch=u64::from_le_bytes(raw[104..112].try_into().unwrap());
  let mut signature=[0u8;64];signature.copy_from_slice(&raw[112..176]);
  let envelope=Envelope{signer,epoch,digest:record.digest,signature};
  let image=&raw[IMAGE_HEADER..n];
  crate::m2_elf::validate_static(image).map_err(|_|Error::Invalid)?;
  if !crate::m2_sha256::matches(image,&record.digest){
   return Err(Error::DigestMismatch);
  }
  self.require_runtime_dependencies(record)?;
  batch4_signature::check(roots,verifier,&envelope,image).map_err(Error::Trust)?;
  if out.len()<image.len(){return Err(Error::Fs(FsError::TooLarge));}
  out[..image.len()].copy_from_slice(image);
  Ok((record,image.len()))
 }
 /// Single-package installs may NOT silently ignore dependencies.
 /// For packages with dependencies use install_from_catalog, which checks every
 /// transitive dependency's persisted minimum version before any writes.
 pub fn install<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package)->Result<Record,Error>{
  if item.deps.iter().any(|&id|id!=0){return Err(Error::MissingDependency);}
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,[0;MAX_DEPS],false)
 }
 /// Validate the entire trusted package dependency closure before updating
 /// any inventory entry. Catalog contents must have been independently
 /// authenticated by the trusted package loader, not supplied by an app.
 pub fn install_from_catalog<V:Verify,const N:usize,const C:usize>(
  &mut self,auth:&Access<N>,owner:Identity,now:u64,
  roots:&[Root],verifier:&V,signed:&Envelope,
  payload:&[u8],catalog:&Catalog<C>,id:u32)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let item=catalog.get(id).ok_or(Error::MissingDependency)?;
  let mut order=[0u32;C];
  let n=catalog.resolve(id,&mut order).map_err(|_|Error::MissingDependency)?;
  for dep in order[..n].iter().copied().filter(|x|*x!=id){
   let required=catalog.get(dep).ok_or(Error::MissingDependency)?;
   let installed=self.read(dep,false)?.ok_or(Error::MissingDependency)?;
   if installed.version<required.version{return Err(Error::MissingDependency);}
  }
  let mut minimums=[0u64;MAX_DEPS];
  for (n,&dep) in item.deps.iter().enumerate(){
   if dep!=0{minimums[n]=catalog.get(dep).ok_or(Error::MissingDependency)?.version;}
  }
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,minimums,false)
 }
 /// Opt-in executable storage: writes exact signed ELF bytes and its
 /// signature together in one durable metadata file, never a detached flag.
 pub fn install_image<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package)->Result<Record,Error>{
  if item.deps.iter().any(|&id|id!=0){return Err(Error::MissingDependency);}
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,[0;MAX_DEPS],true)
 }
 /// Same trusted dependency checks as metadata-only catalog installs.
 pub fn install_image_from_catalog<V:Verify,const N:usize,const C:usize>(
  &mut self,auth:&Access<N>,owner:Identity,now:u64,
  roots:&[Root],verifier:&V,signed:&Envelope,
  payload:&[u8],catalog:&Catalog<C>,id:u32)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let item=catalog.get(id).ok_or(Error::MissingDependency)?;
  let mut order=[0u32;C];
  let n=catalog.resolve(id,&mut order).map_err(|_|Error::MissingDependency)?;
  for dep in order[..n].iter().copied().filter(|x|*x!=id){
   let required=catalog.get(dep).ok_or(Error::MissingDependency)?;
   let installed=self.read(dep,false)?.ok_or(Error::MissingDependency)?;
   if installed.version<required.version{return Err(Error::MissingDependency);}
  }
  let mut minimums=[0u64;MAX_DEPS];
  for (n,&dep) in item.deps.iter().enumerate(){
   if dep!=0{minimums[n]=catalog.get(dep).ok_or(Error::MissingDependency)?.version;}
  }
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,minimums,true)
 }
 fn install_validated<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package,
  minimums:[u64;MAX_DEPS],persist_image:bool)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  if item.id==0||item.id>99||item.version==0||
   item.abi!=crate::m2_sdk::ABI||item.payload_digest!=signed.digest||
   item.rights&!crate::batch6_security::ALL!=0{
   return Err(Error::Invalid);
  }
  for n in 0..MAX_DEPS{
   let dep=item.deps[n];
   if (dep==0&&minimums[n]!=0)||
      (dep!=0&&(dep>99||dep==item.id||minimums[n]==0))||
      (dep!=0&&item.deps[..n].contains(&dep)){
    return Err(Error::Invalid);
   }
  }
  if persist_image{
   if payload.len()>MAX_IMAGE_BYTES{return Err(Error::Fs(FsError::TooLarge));}
   crate::m2_elf::validate_static(payload).map_err(|_|Error::Invalid)?;
   if !crate::m2_sha256::matches(payload,&signed.digest){
    return Err(Error::DigestMismatch);
   }
  }
  // Real verifier MUST verify the actual digest and digital signature;
  // NEVER accept a user-supplied "verified" boolean.
  batch4_signature::check(roots,verifier,signed,payload).map_err(Error::Trust)?;
  if self.read(item.id,true)?.is_some(){return Err(Error::RollbackPending);}
  let previous=self.read(item.id,false)?;
  if previous.is_some_and(|old|item.version<=old.version){
   return Err(Error::Downgrade);
  }
  let next=Record{id:item.id,version:item.version,
   rights:item.rights,digest:signed.digest,deps:item.deps,minimums};
  let p=name(item.id,false)?;
  let mut data=[0u8;MAX_FILE_BYTES];
  data[..LEN].copy_from_slice(&encode(next));
  let size=if persist_image{
   data[..4].copy_from_slice(b"M2P3");
   data[100..104].copy_from_slice(&signed.signer.to_le_bytes());
   data[104..112].copy_from_slice(&signed.epoch.to_le_bytes());
   data[112..176].copy_from_slice(&signed.signature);
   data[176..178].copy_from_slice(&(payload.len() as u16).to_le_bytes());
   data[IMAGE_HEADER..IMAGE_HEADER+payload.len()].copy_from_slice(payload);
   IMAGE_HEADER+payload.len()
  }else{LEN};
  if previous.is_some(){
   let backup=name(item.id,true)?;
   let mut prior=[0u8;MAX_FILE_BYTES];
   let (_,oldsize)=self.read_raw(item.id,false,&mut prior)?
      .ok_or(Error::NotFound)?;
   self.volume.create(path(&backup),&prior[..oldsize]).map_err(Error::Fs)?;
   self.volume.overwrite(path(&p),&data[..size]).map_err(Error::Fs)?;
  }else{
   self.volume.create(path(&p),&data[..size]).map_err(Error::Fs)?;
  }
  Ok(next)
 }
 /// After successful upgrade, retain one rollback checkpoint until the
 /// trusted runtime confirms the new image is genuinely running and healthy.
 /// Confirmation removes the backup to permit the NEXT upgrade.
 pub fn confirm_update<H:RuntimeHealth,const N:usize>(&mut self,
  auth:&Access<N>,owner:Identity,now:u64,id:u32,health:&H)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let old=self.read(id,true)?.ok_or(Error::NotFound)?;
  let active=self.read(id,false)?.ok_or(Error::NotFound)?;
  if active.version<=old.version{return Err(Error::RollbackPending);}
  if !health.healthy_running_image(id,active.version,&active.digest){
   return Err(Error::Denied);
  }
  let backup=name(id,true)?;
  self.volume.unlink(path(&backup)).map_err(Error::Fs)?;
  Ok(active)
 }
 /// Resolve an interrupted upgrade where backup was committed, but the
 /// active package record never changed. NEVER discard a real checkpoint
 /// for a newer active version, which must stay rollback-capable.
 pub fn recover_staged_backup<const N:usize>(&mut self,
  auth:&Access<N>,owner:Identity,now:u64,id:u32)->Result<bool,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let Some(previous)=self.read(id,true)? else{return Ok(false)};
  let active=self.read(id,false)?.ok_or(Error::NotFound)?;
  if active!=previous{return Err(Error::RollbackPending);}
  let backup=name(id,true)?;
  self.volume.unlink(path(&backup)).map_err(Error::Fs)?;
  Ok(true)
 }
 /// Check all installed package metadata rather than relying on an
 /// untrusted or stale catalog supplied during uninstall.
 fn require_dependents_safe(&mut self,id:u32,remaining:Option<u64>)
  ->Result<(),Error>{
  for other in 1..=99{
   if other==id{continue;}
   let Some(record)=self.read(other,false)? else{continue};
   if record.deps==[UNKNOWN_DEP;MAX_DEPS]{return Err(Error::InUse);}
   for n in 0..MAX_DEPS{
    if record.deps[n]==id &&
       remaining.map_or(true,|version|version<record.minimums[n]){
     return Err(Error::InUse);
    }
   }
  }
  Ok(())
 }
 pub fn rollback<const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,id:u32)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let mut backup_bytes=[0u8;MAX_FILE_BYTES];
  let (older,oldsize)=self.read_raw(id,true,&mut backup_bytes)?
   .ok_or(Error::NotFound)?;
  self.require_dependents_safe(id,Some(older.version))?;
  let p=name(id,false)?;
  if self.read(id,false)?.is_none(){return Err(Error::NotFound);}
  self.volume.overwrite(path(&p),&backup_bytes[..oldsize]).map_err(Error::Fs)?;
  let old=name(id,true)?;
  self.volume.unlink(path(&old)).map_err(Error::Fs)?;
  Ok(older)
 }
 pub fn remove<const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,id:u32)->Result<(),Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  // An interrupted remove may leave only the backup; retry must clean it.
  let installed=self.read(id,false)?;
  let backup=self.read(id,true)?;
  if installed.is_none()&&backup.is_none(){return Err(Error::NotFound);}
  self.require_dependents_safe(id,None)?;
  if installed.is_some(){
   let p=name(id,false)?;self.volume.unlink(path(&p)).map_err(Error::Fs)?;
  }
  if backup.is_some(){
   let b=name(id,true)?;self.volume.unlink(path(&b)).map_err(Error::Fs)?;
  }
  Ok(())
 }
}
#[cfg(test)]mod m2_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 const CAP:usize=100;
 struct Disk{blocks:[[u8;SECTOR_BYTES];CAP],fail:bool,writes:usize,fail_at:Option<usize>}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{CAP as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.blocks.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,src:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   if self.fail||self.fail_at==Some(self.writes){return Err(BlockError::Io);}
   *self.blocks.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*src;
   self.writes+=1;
   Ok(())
  }
 }
 struct TestVerifier;
 impl Verify for TestVerifier{
  fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool{
   // Test-only stand-in: absolutely NOT actual Ed25519.
   payload==b"candidate-elf"&&e.signature[0]==9&&e.digest[0]!=0
  }
 }
 fn envelope(version:u8)->Envelope{Envelope{signer:1,epoch:1,digest:[version;32],signature:[9;64]}}
 fn pkg(version:u64)->Package{
  Package{id:7,version,abi:crate::m2_sdk::ABI,rights:crate::batch6_security::FILE_READ,
   deps:[0;4],payload_digest:[version as u8;32]}
 }
 fn actor()->Identity{Identity{pid:12,generation:1}}
 fn auth()->Access<1>{let mut a=Access::new();a.grant(actor(),INSTALL,100).unwrap();a}
 fn roots()->[Root;1]{[Root{signer:1,epoch:1,enabled:true}]}
 fn fresh()->Inventory<Disk>{
  let d=Disk{blocks:[[0;SECTOR_BYTES];CAP],fail:false,writes:0,fail_at:None};
  let mut i=Inventory::from_mounted(HierVfs::format_blank(d).unwrap());
  i.init_directory().unwrap();i
 }
 fn put(i:&mut Inventory<Disk>,owner:Identity,v:u64)->Result<Record,Error>{
  i.install(&auth(),owner,1,&roots(),&TestVerifier,&envelope(v as u8),
   b"candidate-elf",pkg(v))
 }
 #[test]fn m2_verified_versioned_record_persists_update_rollback_remove(){
  let mut i=fresh();
  assert_eq!(put(&mut i,actor(),1).unwrap().version,1);
  assert_eq!(put(&mut i,actor(),1),Err(Error::Downgrade));
  assert_eq!(put(&mut i,actor(),2).unwrap().version,2);
  assert_eq!(put(&mut i,actor(),3),Err(Error::RollbackPending));
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,2);
  assert_eq!(i.rollback(&auth(),actor(),1,7).unwrap().version,1);
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,1);
  i.remove(&auth(),actor(),1,7).unwrap();
  assert_eq!(i.installed(7),Ok(None));
 }
 struct RealHealthTest{good_version:u64}
 impl RuntimeHealth for RealHealthTest{
  fn healthy_running_image(&self,app:u32,version:u64,digest:&[u8;32])->bool{
   app==7&&version==self.good_version&&digest[0]==version as u8
  }
 }
 struct DeniedHealth;
 impl RuntimeHealth for DeniedHealth{
  fn healthy_running_image(&self,_:u32,_:u64,_:&[u8;32])->bool{false}
 }
 #[test]fn m2_confirm_health_releases_checkpoint_and_allows_next_upgrade(){
  let mut i=fresh();
  put(&mut i,actor(),1).unwrap();put(&mut i,actor(),2).unwrap();
  assert_eq!(i.confirm_update(&auth(),actor(),1,7,&DeniedHealth),Err(Error::Denied));
  assert_eq!(i.confirm_update(&auth(),Identity{generation:2,..actor()},
   1,7,&RealHealthTest{good_version:2}),Err(Error::Denied));
  assert_eq!(put(&mut i,actor(),3),Err(Error::RollbackPending));
  assert_eq!(i.confirm_update(&auth(),actor(),1,7,
   &RealHealthTest{good_version:2}).unwrap().version,2);
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(put(&mut i,actor(),3).unwrap().version,3);
  assert_eq!(i.installed(7).unwrap().unwrap().version,3);
 }
 #[test]fn m2_backup_only_after_failed_upgrade_is_recoverable(){
  let mut i=fresh();put(&mut i,actor(),1).unwrap();
  let mut disk=i.into_device();
  disk.fail_at=Some(disk.writes+5); // 4 sectors + metadata for backup; then fail active write
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(put(&mut i,actor(),2),Err(Error::Fs(FsError::Io)));
  let mut disk=i.into_device();disk.fail_at=None;
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,1);
  assert_eq!(put(&mut i,actor(),2),Err(Error::RollbackPending));
  assert_eq!(i.recover_staged_backup(&auth(),actor(),1,7),Ok(true));
  assert_eq!(i.recover_staged_backup(&auth(),actor(),1,7),Ok(false));
  assert_eq!(put(&mut i,actor(),2).unwrap().version,2);
  // A true active upgrade must never be silently reconciled and lost.
  assert_eq!(i.recover_staged_backup(&auth(),actor(),1,7),Err(Error::RollbackPending));
 }
 #[test]fn m2_installer_rejects_unmet_and_old_dependencies_without_writes(){
  let mut i=fresh();
  let dependency=Package{id:5,version:2,abi:crate::m2_sdk::ABI,
   rights:0,deps:[0;4],payload_digest:[2;32]};
  let dependant=Package{id:7,version:3,abi:crate::m2_sdk::ABI,
   rights:0,deps:[5,0,0,0],payload_digest:[3;32]};
  let mut catalog=Catalog::<3>::new();
  catalog.add(dependency).unwrap();catalog.add(dependant).unwrap();
  let generation=i.volume.generation();
  assert_eq!(i.install_from_catalog(&auth(),actor(),1,&roots(),
    &TestVerifier,&envelope(3),b"candidate-elf",&catalog,7),
   Err(Error::MissingDependency));
  assert_eq!(i.volume.generation(),generation);
  assert_eq!(i.install(&auth(),actor(),1,&roots(),&TestVerifier,
   &envelope(3),b"candidate-elf",dependant),Err(Error::MissingDependency));
  assert_eq!(i.volume.generation(),generation);
  // An installed but older dependency must still fail closed.
  let old=Package{version:1,payload_digest:[1;32],..dependency};
  i.install(&auth(),actor(),1,&roots(),&TestVerifier,
    &envelope(1),b"candidate-elf",old).unwrap();
  assert_eq!(i.install_from_catalog(&auth(),actor(),1,&roots(),
    &TestVerifier,&envelope(3),b"candidate-elf",&catalog,7),
   Err(Error::MissingDependency));
  // Upgrade the prerequisite, confirm trusted health and then install.
  i.install(&auth(),actor(),1,&roots(),&TestVerifier,
    &envelope(2),b"candidate-elf",dependency).unwrap();
  assert_eq!(i.install_from_catalog(&auth(),actor(),1,&roots(),
    &TestVerifier,&envelope(3),b"candidate-elf",&catalog,7)
   .unwrap().version,3);
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,3);
 }
 #[test]fn m2_dependency_cycles_cannot_be_installed(){
  let mut i=fresh();
  let a=Package{id:5,version:1,abi:crate::m2_sdk::ABI,rights:0,
   deps:[7,0,0,0],payload_digest:[1;32]};
  let b=Package{id:7,version:1,abi:crate::m2_sdk::ABI,rights:0,
   deps:[5,0,0,0],payload_digest:[1;32]};
  let mut catalog=Catalog::<2>::new();
  catalog.add(a).unwrap();catalog.add(b).unwrap();
  let before=i.volume.generation();
  assert_eq!(i.install_from_catalog(&auth(),actor(),1,&roots(),
   &TestVerifier,&envelope(1),b"candidate-elf",&catalog,5),
   Err(Error::MissingDependency));
  assert_eq!(i.volume.generation(),before);
 }
 #[test]fn m2_wrong_pid_unauthorized_tamper_and_io_fail_closed(){
  let mut i=fresh();
  assert_eq!(put(&mut i,Identity{generation:2,..actor()},1),Err(Error::Denied));
  let bad=Envelope{signature:[0;64],..envelope(1)};
  assert_eq!(i.install(&auth(),actor(),1,&roots(),&TestVerifier,&bad,
   b"candidate-elf",pkg(1)),Err(Error::Trust(batch4_signature::Error::Malformed)));
  put(&mut i,actor(),1).unwrap();
  let mut disk=i.into_device();disk.fail=true;
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(put(&mut i,actor(),2),Err(Error::Fs(FsError::Io)));
  let mut disk=i.into_device();disk.fail=false;
  let mut restored=Inventory::mount(disk).unwrap();
  assert_eq!(restored.installed(7).unwrap().unwrap().version,1);
 }
 #[test]fn m2_persisted_minimums_block_dependents_across_remount(){
  let mut i=fresh();
  let dep=Package{id:5,version:1,abi:crate::m2_sdk::ABI,
    rights:0,deps:[0;4],payload_digest:[1;32]};
  i.install(&auth(),actor(),1,&roots(),&TestVerifier,
    &envelope(1),b"candidate-elf",dep).unwrap();
  let dep2=Package{version:2,payload_digest:[2;32],..dep};
  i.install(&auth(),actor(),1,&roots(),&TestVerifier,
    &envelope(2),b"candidate-elf",dep2).unwrap();
  let app=Package{id:7,version:3,abi:crate::m2_sdk::ABI,
    rights:0,deps:[5,0,0,0],payload_digest:[3;32]};
  let mut catalog=Catalog::<2>::new();
  catalog.add(dep2).unwrap();catalog.add(app).unwrap();
  i.install_from_catalog(&auth(),actor(),1,&roots(),&TestVerifier,
    &envelope(3),b"candidate-elf",&catalog,7).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let installed=i.installed(7).unwrap().unwrap();
  assert_eq!(installed.deps,[5,0,0,0]);
  assert_eq!(installed.minimums,[2,0,0,0]);
  let before=i.volume.generation();
  assert_eq!(i.remove(&auth(),actor(),1,5),Err(Error::InUse));
  assert_eq!(i.rollback(&auth(),actor(),1,5),Err(Error::InUse));
  assert_eq!(i.volume.generation(),before);
  i.remove(&auth(),actor(),1,7).unwrap();
  assert_eq!(i.rollback(&auth(),actor(),1,5).unwrap().version,1);
  i.remove(&auth(),actor(),1,5).unwrap();
 }
 #[test]fn m2_legacy_metadata_unknown_dependencies_fail_closed(){
  let mut i=fresh();put(&mut i,actor(),1).unwrap();
  let mut old=[0u8;LEGACY_LEN];
  old[..4].copy_from_slice(b"M2PK");
  old[4..8].copy_from_slice(&8u32.to_le_bytes());
  old[8..16].copy_from_slice(&1u64.to_le_bytes());
  old[20..52].copy_from_slice(&[8u8;32]);
  i.volume.create("/pkg/p08",&old).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.installed(8).unwrap().unwrap().deps,[UNKNOWN_DEP;MAX_DEPS]);
  let generation=i.volume.generation();
  assert_eq!(i.remove(&auth(),actor(),1,7),Err(Error::InUse));
  assert_eq!(i.volume.generation(),generation);
  i.remove(&auth(),actor(),1,8).unwrap();
  i.remove(&auth(),actor(),1,7).unwrap();
 }

 struct ImageVerifier;
 impl Verify for ImageVerifier{
  fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool{
   payload.len()==124&&payload[120]==7&&e.signature[0]==9&&
    crate::m2_sha256::matches(payload,&e.digest)
  }
 }
 fn image_envelope(payload:&[u8])->Envelope{
  Envelope{signer:1,epoch:1,digest:crate::m2_sha256::digest(payload),
    signature:[9;64]}
 }
 fn image_pkg(version:u64,payload:&[u8])->Package{
  let mut p=pkg(version);
  p.payload_digest=crate::m2_sha256::digest(payload);
  p
 }
 struct AlwaysDeny;
 impl Verify for AlwaysDeny{
  fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}
 }
 #[test]fn m2_signed_elf_image_persists_and_rollback_restores_exact_bytes(){
  let mut i=fresh();
  let first=crate::m2_elf::tests::sample();
  let mut second=first;second[121]=99;
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&first),&first,image_pkg(1,&first)).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;128];
  let (record,n)=i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out).unwrap();
  assert_eq!(record.version,1);assert_eq!(n,124);
  assert_eq!(&out[..n],&first);
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &AlwaysDeny,7,&mut out),Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&second),&second,image_pkg(2,&second)).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out).unwrap().0.version,2);
  assert_eq!(out[121],99);
  i.rollback(&auth(),actor(),1,7).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out).unwrap().0.version,1);
  assert_eq!(&out[..124],&first);
  i.remove(&auth(),actor(),1,7).unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out),Err(Error::NotFound));
 }
 #[test]fn m2_signed_elf_storage_refuses_invalid_stale_and_metadata_only(){
  let mut i=fresh();
  let valid=crate::m2_elf::tests::sample();
  let mut invalid=valid;invalid[68]=7;
  let before=i.volume.generation();
  assert_eq!(i.install_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,&image_envelope(&invalid),&invalid,image_pkg(1,&invalid)),Err(Error::Invalid));
  assert_eq!(i.volume.generation(),before);
  assert_eq!(i.install_image(&auth(),Identity{generation:2,..actor()},1,
   &roots(),&ImageVerifier,&image_envelope(&valid),&valid,image_pkg(1,&valid)),Err(Error::Denied));
  assert_eq!(i.volume.generation(),before);
  let mut out=[0u8;128];
  put(&mut i,actor(),1).unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out),Err(Error::ImageMissing));
  assert_eq!(i.verified_image(&auth(),Identity{generation:2,..actor()},1,
   &roots(),&ImageVerifier,7,&mut out),Err(Error::Denied));
 }
 #[test]fn m2_saved_elf_is_rechecked_if_storage_bytes_are_replaced(){
  let mut i=fresh();
  let first=crate::m2_elf::tests::sample();
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&first),&first,image_pkg(1,&first)).unwrap();
  // Simulate malicious use of an otherwise checksummed storage backend:
  // writing a new, CRC-valid image with a corrupted signed payload.
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,size)=i.read_raw(7,false,&mut raw).unwrap().unwrap();
  raw[IMAGE_HEADER+120]=19;
  i.volume.overwrite("/pkg/p07",&raw[..size]).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;128];
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &ImageVerifier,7,&mut out),Err(Error::DigestMismatch));
 }

 #[test]fn m2_real_sha256_digest_gate_prevents_invalid_install_writes(){
  let mut inv=fresh();
  let image=crate::m2_elf::tests::sample();
  let before=inv.volume.generation();
  assert_eq!(inv.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &envelope(1),&image,pkg(1)),Err(Error::DigestMismatch));
  assert_eq!(inv.volume.generation(),before);
  assert_eq!(inv.installed(7),Ok(None));
 }
 #[test]fn m2_saved_transitive_dependency_downgrade_blocks_execution(){
  let mut inv=fresh();
  let dep=Package{id:8,version:2,abi:crate::m2_sdk::ABI,rights:0,
   deps:[0;4],payload_digest:[2;32]};
  let image=crate::m2_elf::tests::sample();
  let root=Package{id:7,version:1,abi:crate::m2_sdk::ABI,rights:0,
   deps:[8,0,0,0],payload_digest:crate::m2_sha256::digest(&image)};
  let mut catalog=Catalog::<3>::new();
  catalog.add(dep).unwrap();catalog.add(root).unwrap();
  inv.install(&auth(),actor(),1,&roots(),&TestVerifier,&envelope(2),
    b"candidate-elf",dep).unwrap();
  inv.install_image_from_catalog(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&image),&image,&catalog,7).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  assert!(inv.verified_image(&auth(),actor(),1,&roots(),
    &ImageVerifier,7,&mut out).is_ok());
  // Simulated CRC-valid downgrade of an installed dependency.
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,length)=inv.read_raw(8,false,&mut raw).unwrap().unwrap();
  raw[8..16].copy_from_slice(&1u64.to_le_bytes());
  inv.volume.overwrite("/pkg/p08",&raw[..length]).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
    &ImageVerifier,7,&mut out),Err(Error::MissingDependency));
 }
 #[test]fn m2_revoked_root_cannot_launch_persisted_image(){
  let mut inv=fresh();
  let image=crate::m2_elf::tests::sample();
  inv.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&image),&image,image_pkg(1,&image)).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  let revoked=[Root{signer:1,epoch:1,enabled:false}];
  assert_eq!(inv.verified_image(&auth(),actor(),1,&revoked,&ImageVerifier,
    7,&mut out),Err(Error::Trust(batch4_signature::Error::Untrusted)));
 }

}
