//! M2P5/P6 domain-separated canonical *manifest* signing transcript.
//! Prior M2P3/4 signatures bound the ELF digest but did not authenticate
//! version, requested rights or dependency minimums. This transcript binds
//! those fields together so tampering after remount changes the verifier input.
//! This is SHA-256 and an INJECTED signature verifier, not on-guest Ed25519.
//! Real Ed25519 and authenticated key provisioning are still mandatory.
use crate::m2_inventory::Record;
use crate::m2_sdk::{ABI,MAX_DEPS};
use crate::batch4_signature::{self,Envelope,Root,Verify};
const DOMAIN:&[u8]=b"IndianOS/M2/SignedManifest/V1\0";
pub const CANONICAL_BYTES:usize=DOMAIN.len()+2+4+8+4+8+4+32+MAX_DEPS*4+MAX_DEPS*8;
pub fn canonical(record:Record,signer:u32,epoch:u64)->[u8;CANONICAL_BYTES]{
 let mut bytes=[0u8;CANONICAL_BYTES];
 let mut n=0;
 macro_rules! put {($a:expr)=>{{
  let value=$a;
  bytes[n..n+value.len()].copy_from_slice(value);
  n+=value.len();
 }}}
 put!(DOMAIN);
 put!(&ABI.to_le_bytes());
 put!(&signer.to_le_bytes());
 put!(&epoch.to_le_bytes());
 put!(&record.id.to_le_bytes());
 put!(&record.version.to_le_bytes());
 put!(&record.rights.to_le_bytes());
 put!(&record.digest);
 for dep in record.deps{put!(&dep.to_le_bytes());}
 for min in record.minimums{put!(&min.to_le_bytes());}
 debug_assert_eq!(n,CANONICAL_BYTES);
 bytes
}
pub fn digest(record:Record,signer:u32,epoch:u64)->[u8;32]{
 crate::m2_sha256::digest(&canonical(record,signer,epoch))
}
/// For signed M2P5/P6 images, Envelope.digest is the digest of canonical
/// metadata; Record.digest is independently checked against actual ELF bytes.
pub fn check<V:Verify>(roots:&[Root],verifier:&V,
 envelope:&Envelope,record:Record)->Result<(),batch4_signature::Error>{
 if envelope.digest!=digest(record,envelope.signer,envelope.epoch){
  return Err(batch4_signature::Error::InvalidSignature);
 }
 let signed=canonical(record,envelope.signer,envelope.epoch);
 batch4_signature::check(roots,verifier,envelope,&signed)
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::batch6_security::{FILE_READ,SOCKET};
 fn rec()->Record{Record{id:3,version:5,rights:FILE_READ|SOCKET,
  digest:[17;32],deps:[1,2,0,0],minimums:[10,8,0,0]}}
 #[test]fn m2_manifest_canonical_binds_version_rights_dependency_and_signer_epoch(){
  let base=digest(rec(),9,3);
  assert_ne!(base,digest(Record{version:6,..rec()},9,3));
  assert_ne!(base,digest(Record{rights:FILE_READ,..rec()},9,3));
  assert_ne!(base,digest(Record{digest:[18;32],..rec()},9,3));
  assert_ne!(base,digest(Record{deps:[1,4,0,0],..rec()},9,3));
  assert_ne!(base,digest(Record{minimums:[11,8,0,0],..rec()},9,3));
  assert_ne!(base,digest(Record{id:4,..rec()},9,3));
  assert_ne!(base,digest(rec(),10,3));
  assert_ne!(base,digest(rec(),9,4));
  assert_eq!(canonical(rec(),9,3).len(),CANONICAL_BYTES);
 }
 struct TestOnly;
 impl Verify for TestOnly{
  fn verify_digest_and_signature(&self,e:&Envelope,bytes:&[u8])->bool{
   // Deliberately fake; not Ed25519. Signature is merely a test marker.
   bytes==canonical(rec(),9,3)&&e.digest==digest(rec(),9,3)&&
    e.signer==9&&e.epoch==3&&e.signature==[8;64]
  }
 }
 #[test]fn m2_manifest_verifier_rejects_crc_valid_metadata_tampering(){
  let e=Envelope{signer:9,epoch:3,digest:digest(rec(),9,3),signature:[8;64]};
  let roots=[Root{signer:9,epoch:3,enabled:true}];
  assert_eq!(check(&roots,&TestOnly,&e,rec()),Ok(()));
  assert_eq!(check(&roots,&TestOnly,&e,Record{version:6,..rec()}),
   Err(batch4_signature::Error::InvalidSignature));
  assert_eq!(check(&roots,&TestOnly,&e,Record{rights:0,..rec()}),
   Err(batch4_signature::Error::InvalidSignature));
  assert_eq!(check(&roots,&TestOnly,&e,Record{minimums:[0;4],..rec()}),
   Err(batch4_signature::Error::InvalidSignature));
  let revoked=[Root{enabled:false,..roots[0]}];
  assert_eq!(check(&revoked,&TestOnly,&e,rec()),
   Err(batch4_signature::Error::Untrusted));
 }
}
