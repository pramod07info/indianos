//! Batch 5 tasks 6,7,8,15: verified-package + explicit-consent +
//! durable-store adapter. Crypto/disk are supplied by independently reviewed
//! backends, not an LLM-provided boolean. Never runs arbitrary binaries.
use crate::batch4_signature::{self,Root,Envelope,Verify};
use crate::batch4_approval::{Consent,Error as ConsentError};
use crate::batch4_rollback::{Journal,Error as JournalError};
#[derive(Debug,PartialEq,Eq)]
pub enum Error{Invalid,Trust(batch4_signature::Error),
 Consent(ConsentError),Journal(JournalError),Disk,NotReady}
pub trait PackageStore{
 /// Stage and synchronously flush a verified payload to the inactive slot.
 fn stage_and_flush(&mut self,app:u32,payload:&[u8])->Result<(),()>;
 /// Atomically switch the slot, flushing before success.
 fn activate_and_flush(&mut self,app:u32)->Result<(),()>;
 /// Best-effort cleanup of an uncommitted inactive slot.
 fn abort_staging(&mut self,app:u32);
}
pub struct Installer{
 approval:Option<Consent>,journal:Journal,app:u32,
 owner:u32,generation:u64,digest:[u8;32],
 signer:u32,signer_epoch:u64,signature:[u8;64],
}
impl Installer{
 pub fn new(running_image:u32)->Self{
  Self{approval:None,journal:Journal::new(running_image),
   app:0,owner:0,generation:0,digest:[0;32],
   signer:0,signer_epoch:0,signature:[0;64]}
 }
 pub fn request<V:Verify>(&mut self,roots:&[Root],verifier:&V,
  envelope:&Envelope,payload:&[u8],app:u32,owner:u32,
  generation:u64,permissions:u32)->Result<(),Error>{
  if self.approval.is_some()||app==0||app==self.journal.active()||
   owner==0||generation==0{return Err(Error::Invalid);}
  batch4_signature::check(roots,verifier,envelope,payload).map_err(Error::Trust)?;
  let consent=Consent::new(owner,generation,envelope.digest,permissions)
   .map_err(Error::Consent)?;
  self.approval=Some(consent);
  self.app=app;self.owner=owner;self.generation=generation;
  self.digest=envelope.digest;self.signer=envelope.signer;
  self.signer_epoch=envelope.epoch;self.signature=envelope.signature;
  Ok(())
 }
 pub fn approve(&mut self,owner:u32,generation:u64,digest:[u8;32],
  yes:bool,granted:u32)->Result<(),Error>{
  self.approval.as_mut().ok_or(Error::NotReady)?
   .respond(owner,generation,digest,yes,granted).map_err(Error::Consent)
 }
 /// Re-checks trust root, signature AND payload on the exact bytes about to
 /// be staged. Cannot be satisfied with a caller-provided "verified" flag.
 pub fn install<S:PackageStore,V:Verify>(&mut self,store:&mut S,
  roots:&[Root],verifier:&V,envelope:&Envelope,payload:&[u8],
  owner:u32,generation:u64)->Result<u32,Error>{
  if owner!=self.owner||generation!=self.generation||
   envelope.digest!=self.digest||envelope.signer!=self.signer||
   envelope.epoch!=self.signer_epoch||envelope.signature!=self.signature||
   !self.approval.as_ref().is_some_and(|c|c.authorized(owner,generation,self.digest)){
   return Err(Error::NotReady);
  }
  batch4_signature::check(roots,verifier,envelope,payload).map_err(Error::Trust)?;
  let app=self.app;
  store.stage_and_flush(app,payload).map_err(|_|Error::Disk)?;
  if let Err(e)=self.journal.stage(app,true){
   store.abort_staging(app);return Err(Error::Journal(e));
  }
  if let Err(e)=self.journal.activate(true,2){
   store.abort_staging(app);let _=self.journal.rollback();
   return Err(Error::Journal(e));
  }
  if store.activate_and_flush(app).is_err(){
   store.abort_staging(app);
   let _=self.journal.rollback();return Err(Error::Disk);
  }
  self.approval=None;Ok(app)
 }
 pub fn active_image(&self)->u32{self.journal.active()}
 pub fn boot_health(&mut self,healthy:bool)->Result<(),Error>{
  self.journal.confirm(healthy).map_err(Error::Journal)
 }
 pub fn recover_failed_boot(&mut self)->Result<(),Error>{
  self.journal.rollback().map_err(Error::Journal)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 struct Deny;impl Verify for Deny{
  fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}
 }
 struct Allow;impl Verify for Allow{
  fn verify_digest_and_signature(&self,_:&Envelope,bytes:&[u8])->bool{bytes==b"payload"}
 }
 struct Store{active:u32,staged:u32,fail:bool}
 impl PackageStore for Store{
  fn stage_and_flush(&mut self,app:u32,_:&[u8])->Result<(),()>{
   self.staged=app;Ok(())
  }
  fn activate_and_flush(&mut self,app:u32)->Result<(),()>{
   if self.fail{return Err(());}self.active=app;Ok(())
  }
  fn abort_staging(&mut self,_:u32){self.staged=0;}
 }
 fn envelope()->Envelope{Envelope{signer:1,epoch:2,digest:[3;32],signature:[4;64]}}
 fn roots()->[Root;1]{[Root{signer:1,epoch:2,enabled:true}]}
 #[test]fn payload_mutation_and_no_consent_are_rejected(){
  let mut i=Installer::new(10);let mut s=Store{active:10,staged:0,fail:false};
  assert_eq!(i.request(&roots(),&Deny,&envelope(),b"payload",11,2,4,1),
   Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
  i.request(&roots(),&Allow,&envelope(),b"payload",11,2,4,1).unwrap();
  assert_eq!(i.install(&mut s,&roots(),&Allow,&envelope(),b"payload",2,4),
   Err(Error::NotReady));
  i.approve(2,4,[3;32],true,1).unwrap();
  assert_eq!(i.install(&mut s,&roots(),&Allow,&envelope(),b"mutated",2,4),
   Err(Error::Trust(batch4_signature::Error::InvalidSignature)));
  assert_eq!(i.install(&mut s,&roots(),&Allow,&envelope(),b"payload",2,4),Ok(11));
  assert_eq!(s.active,11);i.boot_health(true).unwrap();
 }
 #[test]fn failed_disk_activation_restores_prior_slot_policy(){
  let mut i=Installer::new(10);
  i.request(&roots(),&Allow,&envelope(),b"payload",11,2,4,0).unwrap();
  i.approve(2,4,[3;32],true,0).unwrap();
  let mut s=Store{active:10,staged:0,fail:true};
  assert_eq!(i.install(&mut s,&roots(),&Allow,&envelope(),b"payload",2,4),
   Err(Error::Disk));
  assert_eq!(i.active_image(),10);assert_eq!(s.active,10);
 }
}