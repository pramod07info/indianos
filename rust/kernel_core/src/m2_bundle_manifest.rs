//! M2 Phase 5A large signed package-manifest validation.
//!
//! A blob store's SHA-256 protects bytes but is NEVER an installation
//! authorization. A signed package binds the stored content hash to
//! app ID, ABI, version, requested rights and dependency minimums.
//! No executable permission is granted without the existing trusted
//! Ed25519 publisher registry and the separate kernel consent ledger.
//! Large signed ELF loading into ring3 is a future bounded milestone.
use crate::batch4_signature::{self,Envelope,Root,Verify};
use crate::batch6_security::ALL;
use crate::m2_sdk::{ABI,MAX_DEPS};
use crate::m2_blob_store::{BlobStore,BundleSource,Info,Error as StorageError,CAPACITY};
use crate::block::BlockDevice;
const DOMAIN:&[u8]=b"IndianOS/M2/LargeBundle/V1\0";
pub const TRANSCRIPT_BYTES:usize=
 DOMAIN.len()+2+4+8+4+4+32+MAX_DEPS*4+MAX_DEPS*8+4+8;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BundleManifest{
 pub app:u32,pub version:u64,pub length:u32,pub digest:[u8;32],
 pub rights:u32,pub deps:[u32;MAX_DEPS],pub minimums:[u64;MAX_DEPS],
 pub signer:u32,pub epoch:u64,
}
pub struct SignedBundle{pub manifest:BundleManifest,pub signature:[u8;64]}
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Invalid,Unauthorized,Signature(batch4_signature::Error),
 Storage(StorageError)}
impl BundleManifest{
 pub fn canonical(&self)->[u8;TRANSCRIPT_BYTES]{
  let mut b=[0u8;TRANSCRIPT_BYTES];let mut at=0usize;
  macro_rules! put {($val:expr)=>{{
   let data=$val;
   b[at..at+data.len()].copy_from_slice(data);
   at+=data.len();
  }}}
  put!(DOMAIN);
  put!(&ABI.to_le_bytes());
  put!(&self.app.to_le_bytes());
  put!(&self.version.to_le_bytes());
  put!(&self.length.to_le_bytes());
  put!(&self.rights.to_le_bytes());
  put!(&self.digest);
  for d in &self.deps{put!(&d.to_le_bytes());}
  for m in &self.minimums{put!(&m.to_le_bytes());}
  put!(&self.signer.to_le_bytes());
  put!(&self.epoch.to_le_bytes());
  debug_assert_eq!(at,TRANSCRIPT_BYTES);
  b
 }
 pub fn validate(&self)->Result<(),Error>{
  if self.app==0||self.version==0||self.length==0||
    self.length as usize>CAPACITY||self.signer==0||self.epoch==0||
    self.rights&!ALL!=0||self.digest==[0;32]{
   return Err(Error::Invalid);
  }
  for i in 0..MAX_DEPS{
   let id=self.deps[i];let min=self.minimums[i];
   if (id==0)!=(min==0)||id==self.app{return Err(Error::Invalid);}
   if id!=0&&self.deps[..i].iter().any(|x|*x==id){
    return Err(Error::Invalid);
   }
  }
  Ok(())
 }
}
impl SignedBundle{
 pub fn envelope(&self)->Envelope{
  Envelope{signer:self.manifest.signer,epoch:self.manifest.epoch,
   digest:crate::m2_sha256::digest(&self.manifest.canonical()),
   signature:self.signature}
 }
 /// Preflight real publisher authority BEFORE any disk writes.
 pub fn verify<V:Verify>(&self,roots:&[Root],verifier:&V)->Result<(),Error>{
  self.manifest.validate()?;
  let envelope=self.envelope();
  batch4_signature::check(roots,verifier,&envelope,
   &self.manifest.canonical()).map_err(Error::Signature)
 }
}
/// Actual durable signed package storage. The returned Info is data-only:
/// before execution the kernel must ALSO enforce consent, dependency graph,
/// W^X ELF mapping, signer revocation and approved syscall permissions.
pub fn stage_verified<D:BlockDevice,R:BundleSource,V:Verify>(
 store:&mut BlobStore<D>,package:&SignedBundle,
 roots:&[Root],verifier:&V,source:&mut R
)->Result<Info,Error>{
 package.verify(roots,verifier)?;
 let m=package.manifest;
 store.stage(m.app,m.version,m.length as usize,m.digest,source)
  .map_err(Error::Storage)
}
/// Preferred on-device installer entry point: the kernel supervisor must
/// hold an unexpired INSTALL grant AND verify publisher authority before
/// the first write to a separately selected disposable/package partition.
/// A real interactive consent UI and protected hardware key root remain
/// separate requirements; callers must not treat this as user approval.
pub fn stage_authorized<D:BlockDevice,R:BundleSource,V:Verify,const N:usize>(
 store:&mut BlobStore<D>,package:&SignedBundle,
 auth:&crate::batch6_security::Access<N>,
 installer:crate::batch5_session::Identity,now:u64,
 roots:&[Root],verifier:&V,source:&mut R
)->Result<Info,Error>{
 auth.require(installer,crate::batch6_security::INSTALL,now)
   .map_err(|_|Error::Unauthorized)?;
 stage_verified(store,package,roots,verifier,source)
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::m2_trust::{Registry,Trusted};
 use crate::m2_ed25519::Ed25519Backend;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,INSTALL,FILE_READ};
 use ed25519_compact::{KeyPair,Seed};
 fn admin()->Identity{Identity{pid:8,generation:1}}
 fn auth()->Access<1>{let mut a=Access::new();a.grant(admin(),INSTALL,80).unwrap();a}
 fn sample()->(SignedBundle,Registry<1>){
  let content=std::vec![0x72;32768];
  let key=KeyPair::from_seed(Seed::new([0x45;32]));
  let mut registry=Registry::<1>::new();
  let mut pubkey=[0u8;32];pubkey.copy_from_slice(&key.pk[..]);
  registry.provision(&auth(),admin(),1,7,2,pubkey).unwrap();
  let m=BundleManifest{app:9,version:1,length:content.len() as u32,
   digest:crate::m2_sha256::digest(&content),rights:FILE_READ,
   deps:[0;MAX_DEPS],minimums:[0;MAX_DEPS],signer:7,epoch:2};
  let signature=key.sk.sign(
   &crate::m2_sha256::digest(&m.canonical()),None);
  let mut signature_bytes=[0;64];signature_bytes.copy_from_slice(&signature[..]);
  (SignedBundle{manifest:m,signature:signature_bytes},registry)
 }
 #[test]fn m2_bundle_ed25519_binds_64k_lengths_content_rights_and_dependency_graph(){
  let (mut signed,registry)=sample();
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  assert_eq!(signed.verify(&roots,&verifier),Ok(()));
  signed.manifest.version=2;
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Signature(
    batch4_signature::Error::InvalidSignature)));
  signed.manifest.version=1;
  signed.manifest.rights=0;
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Signature(
    batch4_signature::Error::InvalidSignature)));
  signed.manifest.rights=FILE_READ;
  signed.manifest.deps=[12,0,0,0];signed.manifest.minimums=[3,0,0,0];
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Signature(
    batch4_signature::Error::InvalidSignature)));
  signed.manifest.deps=[0;4];signed.manifest.minimums=[0;4];
  signed.signature[30]^=1;
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Signature(
    batch4_signature::Error::InvalidSignature)));
 }
 #[test]fn m2_bundle_rejects_bad_metadata_before_publisher_crypto(){
  let (mut signed,registry)=sample();
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  signed.manifest.length=(CAPACITY+1) as u32;
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Invalid));
  signed.manifest.length=512;
  signed.manifest.deps=[8,8,0,0];signed.manifest.minimums=[1,1,0,0];
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Invalid));
  signed.manifest.deps=[0;4];signed.manifest.minimums=[0;4];
  signed.manifest.app=0;
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Invalid));
 }
 #[test]fn m2_authorized_installer_guards_all_writes_and_remounts_package(){
  use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
  use crate::m2_blob_store::{REQUIRED_SECTORS,BundleSource};
  struct Disk{blocks:std::vec::Vec<[u8;SECTOR_BYTES]>,writes:u32}
  impl BlockDevice for Disk{
   fn sector_count(&self)->u64{self.blocks.len() as u64}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
     ->Result<(),BlockError>{
    *out=*self.blocks.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
   }
   fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
     ->Result<(),BlockError>{
    *self.blocks.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*data;
    self.writes+=1;Ok(())
   }
  }
  struct Slice<'a>(&'a [u8]);
  impl BundleSource for Slice<'_>{
   fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()>{
    out.copy_from_slice(self.0.get(offset as usize..offset as usize+out.len()).ok_or(())?);
    Ok(())
   }
  }
  let (signed,registry)=sample();
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  let bytes=[0x72u8;32768];
  let mut store=BlobStore::mount(Disk{
   blocks:std::vec![[0;SECTOR_BYTES];REQUIRED_SECTORS as usize],writes:0
  }).unwrap();
  let denied=Access::<1>::new();
  assert_eq!(stage_authorized(&mut store,&signed,&denied,admin(),1,
    &roots,&verifier,&mut Slice(&bytes)),Err(Error::Unauthorized));
  assert_eq!(store.active(),None);
  let mut bad=SignedBundle{manifest:signed.manifest,signature:signed.signature};
  bad.signature[0]^=1;
  assert!(matches!(stage_authorized(&mut store,&bad,&auth(),admin(),1,
    &roots,&verifier,&mut Slice(&bytes)),Err(Error::Signature(_))));
  assert_eq!(store.active(),None);
  let info=stage_authorized(&mut store,&signed,&auth(),admin(),1,
    &roots,&verifier,&mut Slice(&bytes)).unwrap();
  assert_eq!(info.length,bytes.len() as u32);
  let disk=store.into_device();
  assert!(disk.writes>0);
  let mut remounted=BlobStore::mount(disk).unwrap();
  assert_eq!(remounted.active(),Some(info));
  let mut out=[0u8;12];
  remounted.verified_range(0,&mut out).unwrap();
  assert_eq!(out,[0x72;12]);
 }
 #[test]fn m2_bundle_revoked_publisher_signature_fails_closed(){
  let (signed,mut registry)=sample();
  registry.revoke(&auth(),admin(),1,7).unwrap();
  let roots=[registry.root(7).unwrap()];
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  assert_eq!(signed.verify(&roots,&verifier),Err(Error::Signature(
    batch4_signature::Error::Untrusted)));
 }
}
