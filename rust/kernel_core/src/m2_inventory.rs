//! M2 on-device executable installer using versioned COW manifests,
//! SHA-256 checks and an injected cryptographic signer verifier.
//! M2P3 saves a small ELF in one file; M2P4 stores a bounded overflow
//! extent in an independent checksummed A/B bank for safe rollback.
//! Trust-root provisioning and a REAL guest Ed25519 verifier remain TODO.
//! All calls here must be made by the trusted kernel supervisor.
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
/// M2P4 stores the first bytes with metadata and one independently
/// checksummed overflow file. Two slots preserve both upgrade versions.
const EXT_HEADER:usize=182;
pub const INLINE_IMAGE_BYTES:usize=MAX_FILE_BYTES-EXT_HEADER;
pub const MAX_IMAGE_BYTES:usize=INLINE_IMAGE_BYTES+MAX_FILE_BYTES;
const LEGACY_IMAGE_LIMIT:usize=MAX_FILE_BYTES-IMAGE_HEADER;
/// Earlier records had no dependency provenance. Never assume they had none.
const UNKNOWN_DEP:u32=u32::MAX;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{
 pub id:u32,pub version:u64,pub rights:u32,pub digest:[u8;32],
 pub deps:[u32;MAX_DEPS],pub minimums:[u64;MAX_DEPS],
}
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Denied,Invalid,NotFound,Downgrade,RollbackPending,MissingDependency,
 InUse,ImageMissing,UnsignedManifest,DigestMismatch,Trust(batch4_signature::Error),Fs(FsError)}
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
 let image=b.len()>=IMAGE_HEADER&&
   (&b[..4]==b"M2P3"||&b[..4]==b"M2P5");
 let extended=b.len()>=EXT_HEADER&&
   (&b[..4]==b"M2P4"||&b[..4]==b"M2P6");
 if !legacy&&!current&&!image&&!extended{return Err(Error::Invalid);}
 if image||extended{
  let size=u16::from_le_bytes([b[176],b[177]]) as usize;
  let signer=u32::from_le_bytes(b[100..104].try_into().unwrap());
  let epoch=u64::from_le_bytes(b[104..112].try_into().unwrap());
  if size==0||signer==0||epoch==0||
     b[112..176].iter().all(|&x|x==0){return Err(Error::Invalid);}
  if image{
   if IMAGE_HEADER.checked_add(size)!=Some(b.len())||
      size>LEGACY_IMAGE_LIMIT{return Err(Error::Invalid);}
  }else{
   let rest=u16::from_le_bytes([b[178],b[179]]) as usize;
   if size!=INLINE_IMAGE_BYTES||rest==0||rest>MAX_FILE_BYTES||
      b[180]>1||b[181]!=0||EXT_HEADER+size!=b.len(){
    return Err(Error::Invalid);
   }
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
/// A/B overflow files are never reused while referenced by active or backup.
/// The immutable manifest commits a version by naming exactly one slot.
fn extent_name(id:u32,slot:u8)->Result<[u8;8],Error>{
 if slot>1{return Err(Error::Invalid);}
 let mut p=name(id,false)?;
 p[5]=if slot==0{b'x'}else{b'y'};
 Ok(p)
}
fn extent_slot(manifest:&[u8])->Option<u8>{
 if manifest.len()>=EXT_HEADER&&
   (&manifest[..4]==b"M2P4"||&manifest[..4]==b"M2P6")&&manifest[180]<=1{
  Some(manifest[180])
 }else{None}
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
 /// M2P5/6 attest the mutable permissions, versions, dependencies and
 /// executable content digest. Older image formats are not secure to run.
 pub fn require_signed_manifest(&mut self,id:u32)->Result<(),Error>{
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,n)=self.read_raw(id,false,&mut raw)?.ok_or(Error::NotFound)?;
  if n<IMAGE_HEADER||(&raw[..4]!=b"M2P5"&&&raw[..4]!=b"M2P6"){
   return Err(Error::UnsignedManifest);
  }
  Ok(())
 }
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
  if n<IMAGE_HEADER||(&raw[..4]!=b"M2P3"&&
     &raw[..4]!=b"M2P4"&&&raw[..4]!=b"M2P5"&&
     &raw[..4]!=b"M2P6"){
   return Err(Error::ImageMissing);
  }
  let signer=u32::from_le_bytes(raw[100..104].try_into().unwrap());
  let epoch=u64::from_le_bytes(raw[104..112].try_into().unwrap());
  let mut signature=[0u8;64];signature.copy_from_slice(&raw[112..176]);
  let metadata_signed=&raw[..4]==b"M2P5"||&raw[..4]==b"M2P6";
  let binding=if metadata_signed{
   crate::m2_manifest::digest(record,signer,epoch)
  }else{record.digest};
  let envelope=Envelope{signer,epoch,digest:binding,signature};
  let mut scratch=[0u8;MAX_IMAGE_BYTES];
  let length=if &raw[..4]==b"M2P4"||&raw[..4]==b"M2P6"{
   let inline=u16::from_le_bytes(raw[176..178].try_into().unwrap()) as usize;
   let extra=u16::from_le_bytes(raw[178..180].try_into().unwrap()) as usize;
   if inline!=INLINE_IMAGE_BYTES||extra==0||extra>MAX_FILE_BYTES||
      inline+extra>scratch.len(){return Err(Error::Invalid);}
   scratch[..inline].copy_from_slice(&raw[EXT_HEADER..n]);
   let slot=raw[180];
   let file=extent_name(id,slot)?;
   let read=self.volume.read(path(&file),&mut scratch[inline..inline+extra])
      .map_err(Error::Fs)?;
   if read!=extra{return Err(Error::Invalid);}
   inline+extra
  }else{
   let len=n-IMAGE_HEADER;
   scratch[..len].copy_from_slice(&raw[IMAGE_HEADER..n]);
   len
  };
  let image=&scratch[..length];
  crate::m2_elf::validate_static(image).map_err(|_|Error::Invalid)?;
  if !crate::m2_sha256::matches(image,&record.digest){
   return Err(Error::DigestMismatch);
  }
  self.require_runtime_dependencies(record)?;
  if metadata_signed{
   crate::m2_manifest::check(roots,verifier,&envelope,record)
    .map_err(Error::Trust)?;
  }else{
   batch4_signature::check(roots,verifier,&envelope,image)
    .map_err(Error::Trust)?;
  }
  if out.len()<length{return Err(Error::Fs(FsError::TooLarge));}
  out[..length].copy_from_slice(image);
  Ok((record,length))
 }
 /// Single-package installs may NOT silently ignore dependencies.
 /// For packages with dependencies use install_from_catalog, which checks every
 /// transitive dependency's persisted minimum version before any writes.
 pub fn install<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package)->Result<Record,Error>{
  if item.deps.iter().any(|&id|id!=0){return Err(Error::MissingDependency);}
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,[0;MAX_DEPS],false,false)
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
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,minimums,false,false)
 }
 /// Opt-in executable storage: writes exact signed ELF bytes and its
 /// signature together in one durable metadata file, never a detached flag.
 pub fn install_image<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package)->Result<Record,Error>{
  if item.deps.iter().any(|&id|id!=0){return Err(Error::MissingDependency);}
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,[0;MAX_DEPS],true,false)
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
  self.install_validated(auth,owner,now,roots,verifier,signed,payload,item,minimums,true,false)
 }
 /// New M2P5/P6: sign the full metadata and ELF hash rather than only
 /// the executable content, preserving old formats for migration.
 pub fn install_manifest_image<V:Verify,const N:usize>(
  &mut self,auth:&Access<N>,owner:Identity,now:u64,
  roots:&[Root],verifier:&V,signed:&Envelope,
  image:&[u8],item:Package)->Result<Record,Error>{
  if item.deps.iter().any(|&dep|dep!=0){return Err(Error::MissingDependency);}
  self.install_validated(auth,owner,now,roots,verifier,signed,image,
   item,[0;MAX_DEPS],true,true)
 }
 /// Signed catalog and exact min-version package installation.
 pub fn install_manifest_image_from_catalog<V:Verify,const N:usize,const C:usize>(
  &mut self,auth:&Access<N>,owner:Identity,now:u64,
  roots:&[Root],verifier:&V,signed:&Envelope,
  image:&[u8],catalog:&Catalog<C>,id:u32)->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  let item=catalog.get(id).ok_or(Error::MissingDependency)?;
  let mut order=[0u32;C];
  let n=catalog.resolve(id,&mut order).map_err(|_|Error::MissingDependency)?;
  for dep in order[..n].iter().copied().filter(|&dep|dep!=id){
   let required=catalog.get(dep).ok_or(Error::MissingDependency)?;
   let installed=self.read(dep,false)?.ok_or(Error::MissingDependency)?;
   if installed.version<required.version{return Err(Error::MissingDependency);}
  }
  let mut minimums=[0u64;MAX_DEPS];
  for (n,dep) in item.deps.iter().copied().enumerate(){
   if dep!=0{minimums[n]=catalog.get(dep).ok_or(Error::MissingDependency)?.version;}
  }
  self.install_validated(auth,owner,now,roots,verifier,signed,image,
   item,minimums,true,true)
 }
 fn install_validated<V:Verify,const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,roots:&[Root],verifier:&V,
  signed:&Envelope,payload:&[u8],item:Package,
  minimums:[u64;MAX_DEPS],persist_image:bool,signed_manifest:bool)
 ->Result<Record,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  if item.id==0||item.id>99||item.version==0||
   item.abi!=crate::m2_sdk::ABI||(!signed_manifest&&item.payload_digest!=signed.digest)||
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
   if !crate::m2_sha256::matches(payload,&item.payload_digest){
    return Err(Error::DigestMismatch);
   }
  }
  // Real verifier MUST verify the actual digest and digital signature;
  // NEVER accept a user-supplied "verified" boolean.
  let next=Record{id:item.id,version:item.version,
   rights:item.rights,digest:item.payload_digest,deps:item.deps,minimums};
  if signed_manifest{
   if !persist_image||signed.digest!=crate::m2_manifest::digest(
      next,signed.signer,signed.epoch){
    return Err(Error::DigestMismatch);
   }
   crate::m2_manifest::check(roots,verifier,signed,next)
    .map_err(Error::Trust)?;
  }else{
   batch4_signature::check(roots,verifier,signed,payload)
    .map_err(Error::Trust)?;
  }
  if self.read(item.id,true)?.is_some(){return Err(Error::RollbackPending);}
  let previous=self.read(item.id,false)?;
  if previous.is_some_and(|old|item.version<=old.version){
   return Err(Error::Downgrade);
  }
  let p=name(item.id,false)?;
  let mut data=[0u8;MAX_FILE_BYTES];
  data[..LEN].copy_from_slice(&encode(next));
  let overflow=persist_image&&payload.len()>LEGACY_IMAGE_LIMIT;
  let size=if persist_image{
   data[100..104].copy_from_slice(&signed.signer.to_le_bytes());
   data[104..112].copy_from_slice(&signed.epoch.to_le_bytes());
   data[112..176].copy_from_slice(&signed.signature);
   if overflow{
    // The old M2P4 slot stays intact during upgrades/rollback. Commit
    // the new overflow bank BEFORE its referencing manifest is published.
    let mut original=[0u8;MAX_FILE_BYTES];
    let former_slot=if self.read_raw(item.id,false,&mut original)?.is_some(){
     extent_slot(&original)
    }else{None};
    let slot=former_slot.map_or(0,|n|1-n);
    let file=extent_name(item.id,slot)?;
    let inline=INLINE_IMAGE_BYTES;
    let extra=payload.len()-inline;
    if extra==0||extra>MAX_FILE_BYTES{return Err(Error::Fs(FsError::TooLarge));}
    data[..4].copy_from_slice(if signed_manifest{b"M2P6"}else{b"M2P4"});
    data[176..178].copy_from_slice(&(inline as u16).to_le_bytes());
    data[178..180].copy_from_slice(&(extra as u16).to_le_bytes());
    data[180]=slot;data[181]=0;
    data[EXT_HEADER..EXT_HEADER+inline].copy_from_slice(&payload[..inline]);
    // Reusing an unreferenced slot after crash/reboot is safe: old active
    // and rollback manifests never point at this bank.
    match self.volume.create(path(&file),&payload[inline..]){
     Ok(())=>{},
     Err(FsError::Exists)=>self.volume.overwrite(path(&file),&payload[inline..])
        .map_err(Error::Fs)?,
     Err(e)=>return Err(Error::Fs(e)),
    }
    EXT_HEADER+inline
   }else{
    data[..4].copy_from_slice(if signed_manifest{b"M2P5"}else{b"M2P3"});
    data[176..178].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    data[IMAGE_HEADER..IMAGE_HEADER+payload.len()].copy_from_slice(payload);
    IMAGE_HEADER+payload.len()
   }
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
 /// Reclaim only overflow banks no longer referenced by an active OR
 /// rollback manifest. Safe to retry after power loss.
 fn clean_unused_extents(&mut self,id:u32)->Result<usize,Error>{
  let mut live=[false;2];
  for old in [false,true]{
   let mut manifest=[0u8;MAX_FILE_BYTES];
   if let Some((_,size))=self.read_raw(id,old,&mut manifest)?{
    if let Some(slot)=extent_slot(&manifest[..size]){
     live[slot as usize]=true;
    }
   }
  }
  let mut removed=0;
  for slot in 0..2{
   if live[slot]{continue;}
   let file=extent_name(id,slot as u8)?;
   match self.volume.unlink(path(&file)){
    Ok(())=>removed+=1,
    Err(FsError::NotFound)=>{},
    Err(e)=>return Err(Error::Fs(e)),
   }
  }
  Ok(removed)
 }
 /// Authorized retry for interrupted removal or checkpoint cleanup.
 /// Reference checks protect both active and rollback extents.
 pub fn reclaim_unused_extents<const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,id:u32)->Result<usize,Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  self.clean_unused_extents(id)
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
  self.clean_unused_extents(id)?;
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
  self.clean_unused_extents(id)?;
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
  self.clean_unused_extents(id)?;
  Ok(older)
 }
 pub fn remove<const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,now:u64,id:u32)->Result<(),Error>{
  auth.require(owner,INSTALL,now).map_err(|_|Error::Denied)?;
  // An interrupted remove may leave only the backup; retry must clean it.
  let installed=self.read(id,false)?;
  let backup=self.read(id,true)?;
  if installed.is_none()&&backup.is_none(){
   // Retried after an interrupted removal: detached banks may remain.
   return if self.clean_unused_extents(id)? > 0{Ok(())}
       else{Err(Error::NotFound)};
  }
  self.require_dependents_safe(id,None)?;
  if installed.is_some(){
   let p=name(id,false)?;self.volume.unlink(path(&p)).map_err(Error::Fs)?;
  }
  if backup.is_some(){
   let b=name(id,true)?;self.volume.unlink(path(&b)).map_err(Error::Fs)?;
  }
  self.clean_unused_extents(id)?;
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
   payload.len()>=124&&payload[120]==7&&e.signature[0]==9&&
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

 fn m2_large_image(size:usize,seed:u8)->std::vec::Vec<u8>{
  let mut image=std::vec![0u8;size];
  image[..124].copy_from_slice(&crate::m2_elf::tests::sample());
  image[96..104].copy_from_slice(&(size as u64).to_le_bytes());
  // ELF filesz must never exceed memsz. The original 124-byte fixture
  // declared memsz=124; extended samples require a full mapped page.
  image[104..112].copy_from_slice(&4096u64.to_le_bytes());
  for (n,b) in image[124..].iter_mut().enumerate(){
   *b=seed.wrapping_add(n as u8);
  }
  image
 }
 #[test]fn m2_extended_image_reboot_upgrade_and_rollback_keeps_both_slots(){
  let mut i=fresh();
  let old=m2_large_image(2800,11);
  let new=m2_large_image(3500,12);
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&old),&old,image_pkg(1,&old)).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&old);
  let mut chunk=[0u8;MAX_FILE_BYTES];
  assert_eq!(i.volume.read("/pkg/x07",&mut chunk),Ok(old.len()-INLINE_IMAGE_BYTES));
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&new),&new,image_pkg(2,&new)).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.rollback_pending(7),Ok(true));
  assert_eq!(i.reclaim_unused_extents(&auth(),actor(),1,7),Ok(0));
  assert_eq!(i.volume.read("/pkg/y07",&mut chunk),Ok(new.len()-INLINE_IMAGE_BYTES));
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&new);
  i.rollback(&auth(),actor(),1,7).unwrap();
  assert_eq!(i.volume.read("/pkg/y07",&mut chunk),Err(FsError::NotFound));
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&old);
  i.remove(&auth(),actor(),1,7).unwrap();
  assert_eq!(i.volume.read("/pkg/x07",&mut chunk),Err(FsError::NotFound));
  assert_eq!(i.installed(7),Ok(None));
 }
 #[test]fn m2_extended_image_crc_valid_corruption_or_missing_extent_denied(){
  let mut i=fresh();
  let image=m2_large_image(2500,22);
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&image),&image,image_pkg(1,&image)).unwrap();
  let mut chunk=[0u8;MAX_FILE_BYTES];
  let count=i.volume.read("/pkg/x07",&mut chunk).unwrap();
  chunk[0]^=7;
  i.volume.overwrite("/pkg/x07",&chunk[..count]).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out),
   Err(Error::DigestMismatch));
  i.volume.unlink("/pkg/x07").unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out),
   Err(Error::Fs(FsError::NotFound)));
 }
 #[test]fn m2_extended_image_failed_new_bank_does_not_change_active(){
  let mut i=fresh();
  let old=m2_large_image(2600,13);
  let update=m2_large_image(2700,14);
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&old),&old,image_pkg(1,&old)).unwrap();
  let mut disk=i.into_device();disk.fail_at=Some(disk.writes);
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&update),&update,image_pkg(2,&update)),Err(Error::Fs(FsError::Io)));
  let mut disk=i.into_device();disk.fail_at=None;
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,1);
  assert_eq!(i.rollback_pending(7),Ok(false));
  let mut out=[0u8;MAX_IMAGE_BYTES];
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&old);
 }
 #[test]fn m2_extended_image_max_bytes_and_confirm_update_reclaim(){
  let mut i=fresh();
  let oversize=std::vec![0u8;MAX_IMAGE_BYTES+1];
  let generation=i.volume.generation();
  assert_eq!(i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&oversize),&oversize,image_pkg(1,&oversize)),
   Err(Error::Fs(FsError::TooLarge)));
  assert_eq!(i.volume.generation(),generation);
  let old=m2_large_image(2500,3);
  let new=m2_large_image(3100,4);
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&old),&old,image_pkg(1,&old)).unwrap();
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&new),&new,image_pkg(2,&new)).unwrap();
  struct Good;
  impl RuntimeHealth for Good{
   fn healthy_running_image(&self,id:u32,version:u64,_:&[u8;32])->bool{id==7&&version==2}
  }
  assert_eq!(i.confirm_update(&auth(),actor(),1,7,&Good).unwrap().version,2);
  let mut buf=[0u8;MAX_FILE_BYTES];
  assert_eq!(i.volume.read("/pkg/x07",&mut buf),Err(FsError::NotFound));
  assert_eq!(i.volume.read("/pkg/y07",&mut buf),Ok(new.len()-INLINE_IMAGE_BYTES));
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&new);
 }

 #[test]fn m2_extended_image_interrupted_manifest_write_recovers_and_reclaims(){
  let mut i=fresh();
  let old=m2_large_image(2750,31);
  let update=m2_large_image(2830,32);
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&old),&old,image_pkg(1,&old)).unwrap();
  let mut disk=i.into_device();
  // COW creates an overflow bank in 5 sector writes, then a backup
  // manifest in 5 writes; fail exactly at active-manifest publication.
  disk.fail_at=Some(disk.writes+10);
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&update),&update,image_pkg(2,&update)),
    Err(Error::Fs(FsError::Io)));
  let mut disk=i.into_device();disk.fail_at=None;
  let mut i=Inventory::mount(disk).unwrap();
  assert_eq!(i.installed(7).unwrap().unwrap().version,1);
  assert_eq!(i.rollback_pending(7),Ok(true));
  assert_eq!(i.recover_staged_backup(&auth(),actor(),1,7),Ok(true));
  assert_eq!(i.rollback_pending(7),Ok(false));
  let mut scratch=[0u8;MAX_FILE_BYTES];
  assert_eq!(i.volume.read("/pkg/y07",&mut scratch),Err(FsError::NotFound));
  let mut out=[0u8;MAX_IMAGE_BYTES];
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&old);
  // Safe retried upgrade now reuses the freed inactive bank.
  i.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
    &image_envelope(&update),&update,image_pkg(2,&update)).unwrap();
  let (_,n)=i.verified_image(&auth(),actor(),1,&roots(),&ImageVerifier,7,&mut out).unwrap();
  assert_eq!(&out[..n],&update);
 }

 fn signed_metadata_record(v:u8,image:&[u8])->Record{
  let item=image_pkg(v as u64,image);
  Record{id:item.id,version:item.version,rights:item.rights,
   digest:item.payload_digest,deps:item.deps,minimums:[0;4]}
 }
 fn signed_metadata_envelope(v:u8,image:&[u8])->Envelope{
  Envelope{signer:1,epoch:1,
   digest:crate::m2_manifest::digest(signed_metadata_record(v,image),1,1),
   signature:[19;64]}
 }
 struct SignedMetadataVerifier;
 impl Verify for SignedMetadataVerifier{
  fn verify_digest_and_signature(&self,e:&Envelope,metadata:&[u8])->bool{
   // Test-only exact allowlist, NOT a signature implementation. A real
   // Ed25519 provider must authenticate ALL canonical fields.
   if e.signer!=1||e.epoch!=1||e.signature!=[19;64]{
    return false;
   }
   let inline=crate::m2_elf::tests::sample();
   let v1=m2_large_image(2600,41);
   let v2=m2_large_image(2800,42);
   let matched=[(1u8,&inline[..]),(1u8,&v1[..]),(2u8,&v2[..])]
    .into_iter().any(|(v,data)|{
     let record=signed_metadata_record(v,data);
     e.digest==crate::m2_manifest::digest(record,1,1)&&
      metadata==crate::m2_manifest::canonical(record,1,1).as_slice()
    });
   matched
  }
 }
 #[test]fn m2_signed_manifest_inline_verified_then_mutable_rights_tamper_denied(){
  let mut inv=fresh();
  let image=crate::m2_elf::tests::sample();
  let envelope=signed_metadata_envelope(1,&image);
  let before=inv.volume.generation();
  let mut bad=Envelope{signer:envelope.signer,epoch:envelope.epoch,
   digest:envelope.digest,signature:envelope.signature};
  bad.digest[0]^=1;
  assert_eq!(inv.install_manifest_image(&auth(),actor(),1,&roots(),
    &SignedMetadataVerifier,&bad,&image,image_pkg(1,&image)),
    Err(Error::DigestMismatch));
  assert_eq!(inv.volume.generation(),before);
  inv.install_manifest_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,&envelope,&image,image_pkg(1,&image)).unwrap();
  assert_eq!(inv.require_signed_manifest(7),Ok(()));
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut out).unwrap().1,image.len());
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,size)=inv.read_raw(7,false,&mut raw).unwrap().unwrap();
  assert_eq!(&raw[..4],b"M2P5");
  raw[16..20].copy_from_slice(
   &(crate::batch6_security::FILE_READ|crate::batch6_security::FILE_WRITE).to_le_bytes());
  inv.volume.overwrite("/pkg/p07",&raw[..size]).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut out),
   Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
 }
 #[test]fn m2_signed_manifest_extended_two_rollback_banks_survive_remount(){
  let mut inv=fresh();
  let v1=m2_large_image(2600,41);
  let v2=m2_large_image(2800,42);
  inv.install_manifest_image(&auth(),actor(),1,&roots(),&SignedMetadataVerifier,
   &signed_metadata_envelope(1,&v1),&v1,image_pkg(1,&v1)).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  let mut buffer=[0u8;MAX_IMAGE_BYTES];
  assert_eq!(inv.require_signed_manifest(7),Ok(()));
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut buffer).unwrap().1,v1.len());
  inv.install_manifest_image(&auth(),actor(),1,&roots(),&SignedMetadataVerifier,
   &signed_metadata_envelope(2,&v2),&v2,image_pkg(2,&v2)).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,size)=inv.read_raw(7,false,&mut raw).unwrap().unwrap();
  assert_eq!(&raw[..4],b"M2P6");
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut buffer).unwrap().1,v2.len());
  assert_eq!(&buffer[..v2.len()],&v2);
  inv.rollback(&auth(),actor(),1,7).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut buffer).unwrap().1,v1.len());
  assert_eq!(&buffer[..v1.len()],&v1);
  assert_eq!(inv.rollback_pending(7),Ok(false));
  // The metadata-only and older unbound image formats must never satisfy
  // the approved-native-execution format gate after migration.
  inv.remove(&auth(),actor(),1,7).unwrap();
  inv.install_image(&auth(),actor(),1,&roots(),&ImageVerifier,
   &image_envelope(&v1),&v1,image_pkg(1,&v1)).unwrap();
  assert_eq!(inv.require_signed_manifest(7),Err(Error::UnsignedManifest));
 }
 #[test]fn m2_signed_manifest_version_or_dependency_tamper_denied(){
  let mut inv=fresh();
  let image=crate::m2_elf::tests::sample();
  inv.install_manifest_image(&auth(),actor(),1,&roots(),&SignedMetadataVerifier,
   &signed_metadata_envelope(1,&image),&image,image_pkg(1,&image)).unwrap();
  let mut bytes=[0u8;MAX_FILE_BYTES];
  let (_,size)=inv.read_raw(7,false,&mut bytes).unwrap().unwrap();
  bytes[8..16].copy_from_slice(&2u64.to_le_bytes());
  inv.volume.overwrite("/pkg/p07",&bytes[..size]).unwrap();
  let mut inv=Inventory::mount(inv.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  assert_eq!(inv.verified_image(&auth(),actor(),1,&roots(),
   &SignedMetadataVerifier,7,&mut out),
   Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
 }

 struct CatalogOnlyVerifier;
 impl Verify for CatalogOnlyVerifier{
  fn verify_digest_and_signature(&self,e:&Envelope,metadata:&[u8])->bool{
   let image=crate::m2_elf::tests::sample();
   let wanted=Record{id:7,version:1,rights:crate::batch6_security::FILE_READ,
    digest:crate::m2_sha256::digest(&image),
    deps:[8,0,0,0],minimums:[2,0,0,0]};
   e.signer==1&&e.epoch==1&&e.signature==[27;64]&&
    e.digest==crate::m2_manifest::digest(wanted,1,1)&&
    metadata==crate::m2_manifest::canonical(wanted,1,1).as_slice()
  }
 }
 #[test]fn m2_signed_manifest_dependency_minimum_and_identity_tamper_denied(){
  let mut i=fresh();
  let dependency=Package{id:8,version:2,abi:crate::m2_sdk::ABI,rights:0,
   deps:[0;4],payload_digest:[2;32]};
  i.install(&auth(),actor(),1,&roots(),&TestVerifier,&envelope(2),
   b"candidate-elf",dependency).unwrap();
  let image=crate::m2_elf::tests::sample();
  let root=Package{id:7,version:1,abi:crate::m2_sdk::ABI,
   rights:crate::batch6_security::FILE_READ,deps:[8,0,0,0],
   payload_digest:crate::m2_sha256::digest(&image)};
  let mut catalog=Catalog::<3>::new();
  catalog.add(dependency).unwrap();
  catalog.add(root).unwrap();
  let signed_record=Record{id:7,version:1,rights:root.rights,
   digest:root.payload_digest,deps:root.deps,minimums:[2,0,0,0]};
  let signed=Envelope{signer:1,epoch:1,
   digest:crate::m2_manifest::digest(signed_record,1,1),
   signature:[27;64]};
  i.install_manifest_image_from_catalog(&auth(),actor(),1,&roots(),
   &CatalogOnlyVerifier,&signed,&image,&catalog,7).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  let mut out=[0u8;MAX_IMAGE_BYTES];
  assert!(i.verified_image(&auth(),actor(),1,&roots(),
   &CatalogOnlyVerifier,7,&mut out).is_ok());
  let mut raw=[0u8;MAX_FILE_BYTES];
  let (_,count)=i.read_raw(7,false,&mut raw).unwrap().unwrap();
  // A CRC-valid edit lowers the dependency minimum while preserving the
  // same ELF image and signature bytes. The graph still resolves, but
  // the canonical metadata signature MUST now fail.
  raw[68..76].copy_from_slice(&1u64.to_le_bytes());
  i.volume.overwrite("/pkg/p07",&raw[..count]).unwrap();
  let mut i=Inventory::mount(i.into_device()).unwrap();
  assert_eq!(i.verified_image(&auth(),actor(),1,&roots(),
   &CatalogOnlyVerifier,7,&mut out),
   Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
 }

}
