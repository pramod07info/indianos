//! True Ed25519 verification for bare-metal M2P5/M2P6 manifests.
//!
//! Uses pinned, unmodified MIT-licensed rust-ed25519-compact 2.6.0,
//! built with default features disabled, no_std, no allocator or RNG.
//! Crypto verifies the Ed25519 signature over the 32-byte SHA-256
//! domain-separated manifest transcript digest, not arbitrary caller bytes.
//!
//! The kernel MUST obtain publisher public keys from a separately
//! authenticated boot trust root. An embedded QEMU TEST key is NOT such a root.
use crate::m2_trust::SignatureBackend;
use ed25519_compact::{PublicKey,Signature};

#[derive(Clone,Copy,Debug,Default)]
pub struct Ed25519Backend;
impl SignatureBackend for Ed25519Backend{
 fn verify_ed25519(
  &self,public_key:&[u8;32],digest:&[u8;32],signature:&[u8;64]
 )->bool{
  PublicKey::new(*public_key)
    .verify(digest, &Signature::new(*signature)).is_ok()
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::batch4_signature::{Envelope,Error as SigError,Root};
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,INSTALL,FILE_READ};
 use crate::m2_inventory::Record;
 use crate::m2_trust::{Registry,Trusted};
 const RFC_PUB:[u8;32]=[215,90,152,1,130,177,10,183,213,75,254,211,201,100,7,58,14,225,114,243,218,166,35,37,175,2,26,104,247,7,81,26];
 const RFC_SIG:[u8;64]=[229,86,67,0,195,96,172,114,144,134,226,204,128,110,130,138,132,135,127,30,184,229,217,116,216,115,224,101,34,73,1,85,95,184,130,21,144,163,59,172,198,30,57,112,28,249,180,107,210,91,245,240,89,91,190,36,101,81,65,67,142,122,16,11];
 const TEST_PUB:[u8;32]=[72,7,90,89,126,114,26,21,110,46,7,153,222,92,192,197,50,77,198,231,234,241,205,212,98,80,134,142,197,50,21,221];
 const APP_SHA:[u8;32]=[31,249,237,28,20,72,237,102,142,75,222,4,113,138,12,136,142,176,217,197,193,50,162,203,24,158,126,149,86,225,85,81];
 const SIGNED_DIGEST:[u8;32]=[87,36,219,230,36,250,33,125,91,114,139,61,208,16,10,140,125,142,154,109,148,182,31,50,238,134,150,188,115,252,87,206];
 const SIGNATURE:[u8;64]=[63,81,43,0,190,178,223,51,88,189,167,181,91,103,50,139,190,90,11,13,224,164,90,78,55,131,33,51,12,27,158,212,136,7,135,17,212,55,183,170,27,95,27,70,70,158,14,123,37,245,126,11,234,8,37,236,34,83,247,131,173,57,39,5];
 fn administrator()->Identity{Identity{pid:11,generation:1}}
 fn policy()->Access<1>{
  let mut a=Access::new();
  a.grant(administrator(),INSTALL,50).unwrap();a
 }
 fn record()->Record{
  Record{id:9,version:1,rights:0,digest:APP_SHA,
    deps:[0;4],minimums:[0;4]}
 }
 #[test]fn m2_ed25519_rfc8032_empty_message_and_negative_vectors(){
  let key=PublicKey::new(RFC_PUB);
  let signature=Signature::new(RFC_SIG);
  assert_eq!(key.verify(b"",&signature),Ok(()));
  assert!(key.verify(b"malicious",&signature).is_err());
  let mut changed=RFC_SIG;
  changed[0]^=1;
  assert!(key.verify(b"",&Signature::new(changed)).is_err());
  assert!(PublicKey::new([0;32]).verify(b"",&signature).is_err());
  // Canonical scalar S must be < Ed25519 subgroup order.
  let mut noncanonical=RFC_SIG;
  noncanonical[32..].fill(0xff);
  assert!(key.verify(b"",&Signature::new(noncanonical)).is_err());
 }
 #[test]fn m2_ed25519_valid_test_key_signature_over_bound_manifest_digest(){
  let backend=Ed25519Backend;
  let bytes=crate::m2_manifest::canonical(record(),7,2);
  assert_eq!(crate::m2_sha256::digest(&bytes),SIGNED_DIGEST);
  assert!(backend.verify_ed25519(&TEST_PUB,&SIGNED_DIGEST,&SIGNATURE));
  let mut bad=SIGNED_DIGEST;bad[0]^=1;
  assert!(!backend.verify_ed25519(&TEST_PUB,&bad,&SIGNATURE));
  let mut tampered=SIGNATURE;tampered[40]^=1;
  assert!(!backend.verify_ed25519(&TEST_PUB,&SIGNED_DIGEST,&tampered));
  assert!(!backend.verify_ed25519(&RFC_PUB,&SIGNED_DIGEST,&SIGNATURE));
 }
 #[test]fn m2_ed25519_real_registry_signer_rotation_rights_and_epoch_gate(){
  let mut registry=Registry::<1>::new();
  registry.provision(&policy(),administrator(),1,7,2,TEST_PUB).unwrap();
  let backend=Ed25519Backend;
  let envelope=Envelope{signer:7,epoch:2,digest:SIGNED_DIGEST,
    signature:SIGNATURE};
  let verifier=Trusted{registry:&registry,backend:&backend};
  let root=[registry.root(7).unwrap()];
  assert_eq!(crate::m2_manifest::check(&root,&verifier,&envelope,record()),Ok(()));
  assert_eq!(crate::m2_manifest::check(&root,&verifier,
    &envelope,Record{rights:FILE_READ,..record()}),
    Err(SigError::InvalidSignature));
  registry.provision(&policy(),administrator(),1,7,3,TEST_PUB).unwrap();
  let roots=[registry.root(7).unwrap()];
  let verifier=Trusted{registry:&registry,backend:&backend};
  assert_eq!(crate::m2_manifest::check(&roots,&verifier,&envelope,record()),
    Err(SigError::Untrusted));
  registry.revoke(&policy(),administrator(),1,7).unwrap();
  assert_eq!(registry.public_key(7,3),Err(crate::m2_trust::Error::Revoked));
 }
}
