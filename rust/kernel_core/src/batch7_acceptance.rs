//! R041 cross-module acceptance: concrete file bytes + Terminal/Files + UI
//! event lifecycle. Additional assertions prevent policy validators from
//! being mistaken for actual drivers, cross-platform runtimes or audits.
#[cfg(test)]mod tests{
 use crate::batch7_workspace::Workspace;
 use crate::batch5_session::Identity;
 use crate::batch2_launcher::{Launch,Image};
 use crate::batch2_settings::{Settings,Theme};
 use crate::batch7_terminal::{OUTPUT_MAX,Error as ShellError};
 use crate::batch6_security::{Access,FILE_READ,SOCKET};
 use crate::batch6_network::Ports;
 use crate::batch6_apps::{self,Requested,Format,Availability};
 use crate::batch5_accessibility::{self,TextId,Locale};
 use crate::batch4_approval::{Consent,Error as ConsentError};
 use crate::batch4_signature::{self,Envelope,Root,Verify};
 use crate::batch6_recovery::Recovery;

 fn id(pid:u32)->Identity{Identity{pid,generation:3}}
 fn image(pid:u32)->Launch{Launch{pid,generation:3,image:Image{
  image_id:pid,entry:0x1000,mapped_end:0x2000,verified:true}}}
 fn settings()->Settings{Settings{theme:Theme::Dark,pointer_speed:5,key_repeat:10}}
 #[test]fn guest_files_and_terminal_share_data_and_revocation(){
  let mut ws=Workspace::<3,3,3,3,3,3,4>::new();
  ws.initialize(id(1),image(1),id(2),image(2),settings(),42,100)
   .unwrap();
  let mut output=[0u8;OUTPUT_MAX];
  for &c in b"write /docs/log hello"{ws.press_ascii(id(2),1,c).unwrap();}
  assert_eq!(ws.submit(id(2),1,&mut output),Ok(5));
  assert_eq!(&output[..5],b"saved");
  assert_eq!(ws.execute(id(2),1,b"cat /docs/log",&mut output),Ok(5));
  assert_eq!(&output[..5],b"hello");
  assert_eq!(ws.execute(id(2),1,b"ls",&mut output),Ok(10));
  assert_eq!(&output[..10],b"/docs/log\n");
  // Three commands submitted, three owner-bound composited damage events.
  for _ in 0..3{assert_eq!(ws.next_frame().unwrap().owner,id(2));}
  assert!(ws.next_frame().is_err());
  assert_eq!(ws.execute(id(2),100,b"cat /docs/log",&mut output),
   Err(crate::batch7_workspace::Error::Auth(
     crate::batch6_security::Error::Expired)));
  ws.retire(id(2)).unwrap();
  assert!(ws.execute(id(2),1,b"cat /docs/log",&mut output).is_err());
  assert_eq!(ws.disk.count(),1); // Only RAM contents retained, not disk-persistent.
 }
 #[test]fn unrelated_session_cannot_acquire_socket_or_typed_file_access(){
  let a=id(2);let b=id(3);
  let mut rights=Access::<1>::new();
  rights.grant(a,FILE_READ,30).unwrap();
  let mut ports=Ports::<1>::new();
  assert!(ports.bind(&rights,a,8080,1,10).is_err());
  assert!(rights.require(b,FILE_READ,1).is_err());
  assert!(rights.require(a,FILE_READ,30).is_err());
 }
 struct Deny;
 impl Verify for Deny{
  fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}
 }
 #[test]fn untrusted_install_and_unlicensed_cross_platform_code_denied(){
  let envelope=Envelope{signer:7,epoch:1,digest:[5;32],signature:[9;64]};
  let root=Root{signer:7,epoch:1,enabled:true};
  assert_eq!(batch4_signature::check(&[root],&Deny,&envelope,b"pkg"),
   Err(batch4_signature::Error::InvalidSignature));
  let mut c=Consent::new(1,2,[4;32],4).unwrap();
  assert_eq!(c.respond(1,2,[3;32],true,4),Err(ConsentError::Stale));
  let mut elf=[0u8;64];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  assert_eq!(batch6_apps::classify(Requested{format:Format::ElfLinux,
   verified:true,bytes:&elf,mac:None},62),Ok(Availability::RuntimeMissing));
  assert!(batch6_apps::classify(Requested{format:Format::ElfLinux,
   verified:false,bytes:&elf,mac:None},62).is_err());
 }
 #[test]fn release_preflight_never_confuses_labels_and_crash_policy_for_ui(){
  assert_eq!(batch5_accessibility::text(Locale::Hindi,TextId::Files),"फ़ाइलें");
  assert!(batch5_accessibility::check_control(20,30,"Files",true).is_err());
  let mut policy=Recovery::<1>::new(1);
  assert_eq!(policy.fault(id(2),14,0,9),Ok(true));
  assert_eq!(policy.fault(id(2),14,0,10),Ok(false));
  // No desktop screenshot/physical hardware/audit evidence synthesized here.
 }
}