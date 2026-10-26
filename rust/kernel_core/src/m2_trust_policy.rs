//! Phase 4C: offline-root-signed publisher key snapshot, no_std and no heap.
//!
//! UNTRUSTED persistent bytes must pass real Ed25519 BEFORE the registry
//! can be atomically replaced. The independently authenticated boot root
//! and trusted minimum sequence MUST be supplied by secure platform boot
//! services (not by this file or untrusted disk). Without those external
//! anchors a reboot can replay an older, correctly signed snapshot.
//! Test-only boot key fixtures MUST NOT be used for real installs.
use crate::m2_trust::{Anchor,Registry,Error as RegistryError,SignatureBackend};
use crate::m2_ed25519::Ed25519Backend;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};

pub const MAX_PUBLISHERS:usize=4;
const DOMAIN:&[u8]=b"IndianOS/M2/PublisherRoots/V1\0";
pub const TRANSCRIPT_BYTES:usize=DOMAIN.len()+8+4+MAX_PUBLISHERS*45;
pub const SNAPSHOT_BYTES:usize=TRANSCRIPT_BYTES+64;
const EMPTY:Anchor=Anchor{signer:0,epoch:0,public_key:[0;32],enabled:false};
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Malformed,Unauthorized,InvalidSignature,Replay,Registry(RegistryError)}
/// Untrusted by default. Calling parse() NEVER grants publisher authority.
#[derive(Clone,Copy)]
pub struct SignedSnapshot{
 pub sequence:u64,
 pub count:u32,
 pub publishers:[Anchor;MAX_PUBLISHERS],
 pub signature:[u8;64],
}
impl SignedSnapshot{
 pub fn empty()->Self{
  Self{sequence:0,count:0,publishers:[EMPTY;MAX_PUBLISHERS],
   signature:[0;64]}
 }
 pub fn transcript(&self)->[u8;TRANSCRIPT_BYTES]{
  let mut out=[0u8;TRANSCRIPT_BYTES];
  out[..DOMAIN.len()].copy_from_slice(DOMAIN);
  let mut at=DOMAIN.len();
  out[at..at+8].copy_from_slice(&self.sequence.to_le_bytes());at+=8;
  out[at..at+4].copy_from_slice(&self.count.to_le_bytes());at+=4;
  for anchor in &self.publishers{
   out[at..at+4].copy_from_slice(&anchor.signer.to_le_bytes());at+=4;
   out[at..at+8].copy_from_slice(&anchor.epoch.to_le_bytes());at+=8;
   out[at..at+32].copy_from_slice(&anchor.public_key);at+=32;
   out[at]=u8::from(anchor.enabled);at+=1;
  }
  debug_assert_eq!(at,TRANSCRIPT_BYTES);
  out
 }
 pub fn to_bytes(&self)->[u8;SNAPSHOT_BYTES]{
  let mut out=[0u8;SNAPSHOT_BYTES];
  out[..TRANSCRIPT_BYTES].copy_from_slice(&self.transcript());
  out[TRANSCRIPT_BYTES..].copy_from_slice(&self.signature);
  out
 }
 pub fn parse(bytes:&[u8])->Result<Self,Error>{
  if bytes.len()!=SNAPSHOT_BYTES||
    !bytes.starts_with(DOMAIN){return Err(Error::Malformed);}
  let mut at=DOMAIN.len();
  let sequence=u64::from_le_bytes(bytes[at..at+8].try_into().map_err(|_|Error::Malformed)?);at+=8;
  let count=u32::from_le_bytes(bytes[at..at+4].try_into().map_err(|_|Error::Malformed)?);at+=4;
  let mut publishers=[EMPTY;MAX_PUBLISHERS];
  for anchor in &mut publishers{
   anchor.signer=u32::from_le_bytes(bytes[at..at+4].try_into().map_err(|_|Error::Malformed)?);at+=4;
   anchor.epoch=u64::from_le_bytes(bytes[at..at+8].try_into().map_err(|_|Error::Malformed)?);at+=8;
   anchor.public_key.copy_from_slice(&bytes[at..at+32]);at+=32;
   anchor.enabled=match bytes[at]{0=>false,1=>true,_=>return Err(Error::Malformed)};at+=1;
  }
  let mut signature=[0u8;64];
  signature.copy_from_slice(&bytes[at..]);
  Ok(Self{sequence,count,publishers,signature})
 }
 fn validate_layout(&self)->Result<(),Error>{
  if self.sequence==0||self.count==0||self.count as usize>MAX_PUBLISHERS||
    self.signature==[0;64]{return Err(Error::Malformed);}
  let mut last=0u32;
  for (i,a) in self.publishers.iter().enumerate(){
   if i<self.count as usize{
    if a.signer==0||a.signer<=last||a.epoch==0||
      a.public_key==[0;32]||a.public_key==[0xff;32]{
     return Err(Error::Malformed);
    }
    last=a.signer;
   }else if *a!=EMPTY{return Err(Error::Malformed);}
  }
  Ok(())
 }
 /// Fail-closed, transactional publication; caller must get accepted_seq
 /// from independent anti-rollback storage (e.g. TPM monotonic NV index).
 /// An untrusted on-disk sequence counter alone does NOT stop replay.
 pub fn verify_and_install<const N:usize>(
  &self,registry:&mut Registry<MAX_PUBLISHERS>,
  authenticated_boot_public_key:&[u8;32],accepted_seq:u64,
  admin_auth:&Access<N>,administrator:Identity,now:u64
 )->Result<u64,Error>{
  self.validate_layout()?;
  if self.sequence<=accepted_seq{return Err(Error::Replay);}
  admin_auth.require(administrator,INSTALL,now).map_err(|_|Error::Unauthorized)?;
  let digest=crate::m2_sha256::digest(&self.transcript());
  if !Ed25519Backend.verify_ed25519(authenticated_boot_public_key,
       &digest,&self.signature){return Err(Error::InvalidSignature);}
  let mut staged=Registry::<MAX_PUBLISHERS>::new();
  for anchor in self.publishers.iter().take(self.count as usize){
   staged.provision(admin_auth,administrator,now,anchor.signer,
     anchor.epoch,anchor.public_key).map_err(Error::Registry)?;
   if !anchor.enabled{
    staged.revoke(admin_auth,administrator,now,anchor.signer)
      .map_err(Error::Registry)?;
   }
  }
  *registry=staged; // no partial mutation on signature/policy errors
  Ok(self.sequence)
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 const BOOT:[u8;32]=[32,18,203,144,202,96,232,229,216,218,246,110,34,114,210,35,62,4,134,213,87,232,198,97,65,237,137,32,23,125,126,183];
 const PUBLISHER:[u8;32]=[72,7,90,89,126,114,26,21,110,46,7,153,222,92,192,197,50,77,198,231,234,241,205,212,98,80,134,142,197,50,21,221];
 const SIGNATURE:[u8;64]=[119,72,45,124,7,155,22,101,144,76,124,204,209,87,120,49,147,230,213,149,252,255,79,198,234,73,239,233,97,36,149,99,34,40,70,94,244,213,101,150,27,204,8,204,212,144,228,34,26,95,59,112,156,232,160,211,65,178,125,176,175,60,242,11];
 fn admin()->Identity{Identity{pid:1,generation:8}}
 fn access()->Access<1>{let mut a=Access::new();a.grant(admin(),INSTALL,100).unwrap();a}
 pub(crate) fn fixture()->SignedSnapshot{
  let mut policy=SignedSnapshot::empty();
  policy.sequence=3;policy.count=1;
  policy.publishers[0]=Anchor{signer:7,epoch:2,public_key:PUBLISHER,enabled:true};
  policy.signature=SIGNATURE;
  policy
 }
 #[test]fn m2_signed_roots_ed25519_boot_key_and_disk_roundtrip(){
  let original=fixture();
  let bytes=original.to_bytes();
  let decoded=SignedSnapshot::parse(&bytes).unwrap();
  assert_eq!(decoded.to_bytes(),bytes);
  let mut registry=Registry::<MAX_PUBLISHERS>::new();
  assert_eq!(decoded.verify_and_install(
    &mut registry,&BOOT,0,&access(),admin(),1),Ok(3));
  assert_eq!(registry.public_key(7,2),Ok(PUBLISHER));
  assert_eq!(decoded.verify_and_install(
    &mut registry,&BOOT,3,&access(),admin(),1),Err(Error::Replay));
  assert_eq!(registry.public_key(7,2),Ok(PUBLISHER));
 }
 #[test]fn m2_signed_roots_rejects_bad_signature_tampered_version_and_wrong_boot_key(){
  let mut registry=Registry::<MAX_PUBLISHERS>::new();
  let mut changed=fixture();
  changed.sequence=4; // still well-formed, but was not signed.
  assert_eq!(changed.verify_and_install(
    &mut registry,&BOOT,0,&access(),admin(),1),
    Err(Error::InvalidSignature));
  assert_eq!(registry.public_key(7,2),Err(RegistryError::Missing));
  let changed=fixture();
  let mut bytes=changed.to_bytes();bytes[0]=b'x';
  assert!(matches!(SignedSnapshot::parse(&bytes),Err(Error::Malformed)));
  let mut bytes=changed.to_bytes();bytes[DOMAIN.len()+12+44]=2;
  assert_eq!(SignedSnapshot::parse(&bytes).err(),Some(Error::Malformed));
  assert_eq!(changed.verify_and_install(&mut registry,&[9;32],0,&access(),admin(),1),
    Err(Error::InvalidSignature));
  assert_eq!(changed.verify_and_install(
    &mut registry,&BOOT,0,&access(),
    Identity{generation:9,..admin()},1),Err(Error::Unauthorized));
  assert_eq!(registry.public_key(7,2),Err(RegistryError::Missing));
 }
 #[test]fn m2_signed_roots_policy_sort_duplicates_and_disabled_enforced(){
  let mut changed=fixture();
  changed.publishers[0].enabled=false;
  // Tampering with enable status is caught by the Ed25519 signature,
  // even when revoked/disabled keys are otherwise structurally valid.
  let mut registry=Registry::<MAX_PUBLISHERS>::new();
  assert_eq!(changed.verify_and_install(&mut registry,&BOOT,0,&access(),admin(),1),
    Err(Error::InvalidSignature));
  let mut bad=fixture();bad.count=2;
  bad.publishers[1]=bad.publishers[0];
  assert_eq!(bad.verify_and_install(&mut registry,&BOOT,0,&access(),admin(),1),
    Err(Error::Malformed));
  assert_eq!(registry.public_key(7,2),Err(RegistryError::Missing));
 }
}
