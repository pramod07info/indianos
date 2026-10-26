//! M2.4 signer-root POLICY, not a cryptography implementation.
//!
//! A kernel-owned public-key registry prevents stale key epochs, unauthorized
//! rotation, signer replay and revoked signers from reaching a caller-supplied
//! Ed25519 backend. It MUST be populated only from verified/protected boot
//! provenance. No backend is supplied by this module; test backend is fake.
//! Actual on-guest Ed25519, signature binding to all manifest metadata,
//! and tamper-resistant persistence remain release blockers.
use crate::batch4_signature::{Envelope,Root,Verify};
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};
use crate::m2_sha256;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error {Denied,Invalid,Full,Missing,StaleEpoch,Revoked}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Anchor {
 pub signer:u32,
 pub epoch:u64,
 pub public_key:[u8;32],
 pub enabled:bool,
}
/// Fixed-capacity in-memory registry. NO untrusted app can mutate it.
pub struct Registry<const K:usize>{entries:[Option<Anchor>;K]}
impl<const K:usize> Registry<K>{
 pub const fn new()->Self{Self{entries:[None;K]}}
 pub fn get(&self,signer:u32)->Option<Anchor>{
  self.entries.iter().flatten().find(|a|a.signer==signer).copied()
 }
 fn authorized<const N:usize>(
  auth:&Access<N>,administrator:Identity,now:u64
 )->Result<(),Error>{
  auth.require(administrator,INSTALL,now).map_err(|_|Error::Denied)
 }
 pub fn provision<const N:usize>(
  &mut self,auth:&Access<N>,administrator:Identity,now:u64,
  signer:u32,epoch:u64,key:[u8;32]
 )->Result<(),Error>{
  Self::authorized(auth,administrator,now)?;
  if signer==0||epoch==0||key==[0;32]||key==[0xff;32]{
   return Err(Error::Invalid);
  }
  if let Some(slot)=self.entries.iter_mut().find(|x|
    x.is_some_and(|a|a.signer==signer)){
   let old=slot.as_ref().unwrap();
   if epoch<=old.epoch{return Err(Error::StaleEpoch);}
   // Rotation also serves as explicit recovery from revocation; never
   // silently reactivate a previously revoked epoch.
   *slot=Some(Anchor{signer,epoch,public_key:key,enabled:true});
   return Ok(());
  }
  let slot=self.entries.iter_mut().find(|x|x.is_none()).ok_or(Error::Full)?;
  *slot=Some(Anchor{signer,epoch,public_key:key,enabled:true});
  Ok(())
 }
 pub fn revoke<const N:usize>(
  &mut self,auth:&Access<N>,administrator:Identity,now:u64,
  signer:u32
 )->Result<(),Error>{
  Self::authorized(auth,administrator,now)?;
  let slot=self.entries.iter_mut().flatten().find(|a|a.signer==signer)
   .ok_or(Error::Missing)?;
  if !slot.enabled{return Err(Error::Revoked);}
  slot.enabled=false;Ok(())
 }
 /// A real signer may be used only at exactly its current trusted epoch.
 pub fn public_key(&self,signer:u32,epoch:u64)->Result<[u8;32],Error>{
  let slot=self.get(signer).ok_or(Error::Missing)?;
  if !slot.enabled{return Err(Error::Revoked);}
  if epoch!=slot.epoch{return Err(Error::StaleEpoch);}
  Ok(slot.public_key)
 }
 pub fn root(&self,signer:u32)->Option<Root>{
  self.get(signer).map(|a|Root{
   signer:a.signer,epoch:a.epoch,enabled:a.enabled
  })
 }
}
/// Caller MUST provide actual constant-time Ed25519 verification over an
/// authenticated envelope. The current M2 envelope only exposes the payload
/// digest, so metadata (version, rights, dependencies) is NOT yet signed.
pub trait SignatureBackend{
 fn verify_ed25519(
  &self,public_key:&[u8;32],digest:&[u8;32],signature:&[u8;64]
 )->bool;
}
/// Connect a boot-provisioned key registry to the existing signed installer.
/// Both the kernel digest and the exact key epoch must match before invoking
/// the backend. Without a real backend, no production trust is established.
pub struct Trusted<'a,const K:usize,B:SignatureBackend>{
 pub registry:&'a Registry<K>,
 pub backend:&'a B,
}
impl<const K:usize,B:SignatureBackend> Verify for Trusted<'_,K,B>{
 fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool{
  if !m2_sha256::matches(payload,&e.digest){return false;}
  let Ok(key)=self.registry.public_key(e.signer,e.epoch)else{return false;};
  self.backend.verify_ed25519(&key,&e.digest,&e.signature)
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::batch4_signature::{check,Error as SignatureError};
 struct TestOnlyBackend;
 impl SignatureBackend for TestOnlyBackend{
  fn verify_ed25519(&self,key:&[u8;32],digest:&[u8;32],sig:&[u8;64])->bool{
   // Deliberate NON-CRYPTO test stub. No Ed25519 claim.
   *key==[9;32]&&*digest==m2_sha256::digest(b"hello")&&*sig==[7;64]
  }
 }
 fn admin()->Identity{Identity{pid:2,generation:4}}
 fn access()->Access<1>{let mut a=Access::new();
  a.grant(admin(),INSTALL,50).unwrap();a}
 fn envelope(epoch:u64)->Envelope{
  Envelope{signer:3,epoch,digest:m2_sha256::digest(b"hello"),
   signature:[7;64]}
 }
 #[test]fn m2_trust_default_deny_rotation_revocation_stale_epoch_and_replay(){
  let mut r=Registry::<1>::new();
  assert_eq!(r.public_key(3,1),Err(Error::Missing));
  assert_eq!(r.provision(&access(),Identity{generation:5,..admin()},1,
    3,1,[9;32]),Err(Error::Denied));
  r.provision(&access(),admin(),1,3,1,[9;32]).unwrap();
  assert_eq!(r.public_key(3,1),Ok([9;32]));
  assert_eq!(r.provision(&access(),admin(),1,3,1,[9;32]),Err(Error::StaleEpoch));
  assert_eq!(r.provision(&access(),admin(),1,3,0,[9;32]),Err(Error::Invalid));
  assert_eq!(r.provision(&access(),admin(),50,3,2,[9;32]),Err(Error::Denied));
  r.revoke(&access(),admin(),1,3).unwrap();
  assert_eq!(r.public_key(3,1),Err(Error::Revoked));
  assert_eq!(r.revoke(&access(),admin(),1,3),Err(Error::Revoked));
  assert_eq!(r.provision(&access(),admin(),1,3,1,[9;32]),Err(Error::StaleEpoch));
  r.provision(&access(),admin(),1,3,2,[9;32]).unwrap();
  assert_eq!(r.public_key(3,1),Err(Error::StaleEpoch));
  assert_eq!(r.public_key(3,2),Ok([9;32]));
  assert_eq!(r.provision(&access(),admin(),1,4,1,[9;32]),Err(Error::Full));
 }
 #[test]fn m2_trust_signature_backend_requires_exact_key_epoch_sha_and_root(){
  let mut r=Registry::<1>::new();
  r.provision(&access(),admin(),1,3,1,[9;32]).unwrap();
  let backend=TestOnlyBackend;
  let v=Trusted{registry:&r,backend:&backend};
  let root=[r.root(3).unwrap()];
  assert_eq!(check(&root,&v,&envelope(1),b"hello"),Ok(()));
  assert_eq!(check(&root,&v,&envelope(1),b"HELLO"),
    Err(SignatureError::InvalidSignature));
  let mut tampered=envelope(1);tampered.signature[10]^=2;
  assert_eq!(check(&root,&v,&tampered,b"hello"),
    Err(SignatureError::InvalidSignature));
  r.provision(&access(),admin(),1,3,2,[9;32]).unwrap();
  let root=[r.root(3).unwrap()];
  let v=Trusted{registry:&r,backend:&backend};
  assert_eq!(check(&root,&v,&envelope(1),b"hello"),Err(SignatureError::Untrusted));
  assert_eq!(check(&root,&v,&envelope(2),b"hello"),Ok(()));
  r.revoke(&access(),admin(),1,3).unwrap();
  let root=[r.root(3).unwrap()];
  let v=Trusted{registry:&r,backend:&backend};
  assert_eq!(check(&root,&v,&envelope(2),b"hello"),Err(SignatureError::Untrusted));
 }
}
