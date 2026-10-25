//! R038 coordinated host-side release path, NOT on-device compatibility.
#[cfg(test)] mod tests {
 use crate::{batch4_ai_catalog,batch4_approval,batch4_signature,
   batch4_rollback,batch4_linux_abi,batch4_windows_pe,batch4_android,
   batch4_macos_policy,batch4_performance,batch4_release};
 #[test] fn catalog_consent_rollback_and_release_gates(){
   let apps=[batch4_ai_catalog::App{id:7,arch:62,trusted:true,
      permissions:1,risk:1,score:80}];
   assert_eq!(batch4_ai_catalog::recommend(&apps,62,1,2),Ok(7));
   let mut consent=batch4_approval::Consent::new(1,3,[2;32],1).unwrap();
   assert!(!consent.authorized(1,3,[2;32]));
   consent.respond(1,3,[2;32],true,1).unwrap();
   assert!(consent.authorized(1,3,[2;32]));
   struct Deny;impl batch4_signature::Verify for Deny{
      fn verify_digest_and_signature(&self,_:&batch4_signature::Envelope,_:&[u8])->bool{false}
   }
   let envelope=batch4_signature::Envelope{signer:2,epoch:1,
      digest:[3;32],signature:[4;64]};
   assert_eq!(batch4_signature::check(&[batch4_signature::Root{
     signer:2,epoch:1,enabled:true}],&Deny,&envelope,b"application"),
     Err(batch4_signature::Error::InvalidSignature));
   let mut journal=batch4_rollback::Journal::new(5);
   journal.stage(7,true).unwrap();journal.activate(true,1).unwrap();
   journal.boot_attempt().unwrap();journal.rollback().unwrap();
   assert_eq!(journal.active(),5);
   assert_eq!(batch4_linux_abi::classify(400),
      Err(batch4_linux_abi::Error::Unsupported));
   assert_eq!(batch4_windows_pe::parse(&[]),
      Err(batch4_windows_pe::Error::Malformed));
   assert_eq!(batch4_android::first_name(&[]),
      Err(batch4_android::Error::Malformed));
   assert_eq!(batch4_macos_policy::choose(batch4_macos_policy::App{
      source_portable:false,web_app:false,remote_license:false,
      authorized_host:false,proprietary_frameworks:true}),
      Err(batch4_macos_policy::Error::Unauthorized));
   let mut perf=batch4_performance::Monitor::new();
   assert_eq!(perf.check(batch4_performance::Stage::Boot,100,100),
      Err(batch4_performance::Error::NoSamples));
   perf.record(batch4_performance::Stage::Boot,15).unwrap();
   assert!(perf.check(batch4_performance::Stage::Boot,20,20).is_ok());
   let evidence=batch4_release::Evidence{sha:[3;20],
      kernel:batch4_release::Status::Pass,
      filesystem:batch4_release::Status::Pass,
      security:batch4_release::Status::Pass,
      update:batch4_release::Status::Pass,
      compatibility:batch4_release::Status::Pass,
      platform_mask:1,independently_reviewed:false};
   assert_eq!(batch4_release::qualify(evidence,[3;20],1),
      Err(batch4_release::Error::Unreviewed));
 }
}