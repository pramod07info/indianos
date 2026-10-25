//! R038/33: trust-root signature engine gate; crypto MUST be supplied separately.
pub struct Root{pub signer:u32,pub epoch:u64,pub enabled:bool}
pub struct Envelope{pub signer:u32,pub epoch:u64,pub digest:[u8;32],pub signature:[u8;64]}
#[derive(Debug,PartialEq,Eq)]pub enum Error{Malformed,Untrusted,InvalidSignature}
pub trait Verify{fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool;}
pub fn check<V:Verify>(roots:&[Root],verifier:&V,e:&Envelope,payload:&[u8])->Result<(),Error>{
 if e.signer==0||e.epoch==0||e.digest==[0;32]||e.signature==[0;64]||
  payload.is_empty()||payload.len()>1024*1024||roots.len()>32{return Err(Error::Malformed);}
 if !roots.iter().any(|r|r.enabled&&r.signer==e.signer&&r.epoch==e.epoch){return Err(Error::Untrusted);}
 if !verifier.verify_digest_and_signature(e,payload){return Err(Error::InvalidSignature);}
 Ok(())
}
#[cfg(test)]mod tests{use super::*;
 struct Deny;impl Verify for Deny{fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}}
 #[test]fn unsigned_and_revoked_packages_fail(){
  let e=Envelope{signer:1,epoch:2,digest:[1;32],signature:[2;64]};
  assert_eq!(check(&[Root{signer:1,epoch:2,enabled:true}],&Deny,&e,b"pkg"),Err(Error::InvalidSignature));
  assert_eq!(check(&[Root{signer:1,epoch:2,enabled:false}],&Deny,&e,b"pkg"),Err(Error::Untrusted));
 }
}