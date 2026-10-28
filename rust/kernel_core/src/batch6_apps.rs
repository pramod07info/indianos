//! R040 tasks 15–19: deterministic compatibility and AI suggestion
//! routing. Parsing a header never makes an executable actually runnable.
use crate::batch4_ai_catalog::{self,App};
use crate::batch5_compat::{self,Route};
use crate::batch4_macos_policy;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Format{Native,ElfLinux,PeWindows,ApkAndroid,MacOnly}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{NoSuggestion,Unverified,UnknownFormat,Unlicensed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Availability{Native,RuntimeMissing,Web,AuthorizedRemote,PortRequired}
pub struct Requested<'a>{pub format:Format,pub verified:bool,
 pub bytes:&'a[u8],pub mac:Option<batch4_macos_policy::App>}
pub fn suggest(catalog:&[App],architecture:u16,grants:u32,risk:u8)
 ->Result<u32,Error>{
 batch4_ai_catalog::recommend(catalog,architecture,grants,risk)
  .map_err(|_|Error::NoSuggestion)
}
pub fn classify(request:Requested<'_>,host_arch:u16)->Result<Availability,Error>{
 if !request.verified{return Err(Error::Unverified);}
 let route=match request.format{
  Format::Native=>batch5_compat::native(true,host_arch,host_arch),
  Format::ElfLinux=>batch5_compat::linux_x86_64_elf(true,request.bytes),
  Format::PeWindows=>batch5_compat::windows_x86_64(true,request.bytes),
  Format::ApkAndroid=>batch5_compat::android_apk(true,request.bytes),
  Format::MacOnly=>{
   let mac=request.mac.ok_or(Error::Unlicensed)?;
   batch5_compat::macos(mac)
  }
 }.map_err(|_|Error::UnknownFormat)?;
 Ok(match route{
  Route::Native=>Availability::Native,
  Route::NeedsLinuxRuntime|Route::NeedsWin32Runtime|Route::NeedsAndroidRuntime=>
   Availability::RuntimeMissing,
  Route::PortableRebuild=>Availability::PortRequired,
  Route::Web=>Availability::Web,
  Route::AuthorizedRemote=>Availability::AuthorizedRemote,
 })
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn models_cannot_force_unknown_compatibility(){
  let apps=[App{id:1,arch:62,trusted:false,permissions:0,risk:0,score:100},
   App{id:2,arch:62,trusted:true,permissions:1,risk:0,score:60}];
  assert_eq!(suggest(&apps,62,1,2),Ok(2));
  assert_eq!(suggest(&apps,62,0,2),Err(Error::NoSuggestion));
  let mut elf=[0u8;120];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[6]=1;
  elf[16..18].copy_from_slice(&2u16.to_le_bytes());
  elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  elf[20..24].copy_from_slice(&1u32.to_le_bytes());
  elf[32..40].copy_from_slice(&64u64.to_le_bytes());
  elf[52..54].copy_from_slice(&64u16.to_le_bytes());
  elf[54..56].copy_from_slice(&56u16.to_le_bytes());
  elf[56..58].copy_from_slice(&1u16.to_le_bytes());
  elf[64..68].copy_from_slice(&1u32.to_le_bytes());
  elf[68..72].copy_from_slice(&5u32.to_le_bytes());
  assert_eq!(classify(Requested{format:Format::ElfLinux,verified:true,
   bytes:&elf,mac:None},62),Ok(Availability::RuntimeMissing));
  assert_eq!(classify(Requested{format:Format::ElfLinux,verified:false,
   bytes:&elf,mac:None},62),Err(Error::Unverified));
 }
}

/// Orbit R005: preflight multiple signed application candidates before any
/// install decision. No executable parser can turn an absent Windows, Linux,
/// Android or macOS runtime into working binary compatibility.
pub fn classify_batch(requests:&[Requested<'_>],host_arch:u16,
 out:&mut[Availability])->Result<usize,Error>{
 if requests.is_empty()||out.len()<requests.len(){
  return Err(Error::UnknownFormat);
 }
 for request in requests{
  let _=classify(Requested{format:request.format,verified:request.verified,
    bytes:request.bytes,mac:request.mac.as_ref().map(|m|batch4_macos_policy::App{
     source_portable:m.source_portable,web_app:m.web_app,
     remote_license:m.remote_license,authorized_host:m.authorized_host,
     proprietary_frameworks:m.proprietary_frameworks
    })},host_arch)?;
 }
 for (i,request) in requests.iter().enumerate(){
  out[i]=classify(Requested{format:request.format,verified:request.verified,
    bytes:request.bytes,mac:request.mac.as_ref().map(|m|batch4_macos_policy::App{
     source_portable:m.source_portable,web_app:m.web_app,
     remote_license:m.remote_license,authorized_host:m.authorized_host,
     proprietary_frameworks:m.proprietary_frameworks
    })},host_arch)?;
 }
 Ok(requests.len())
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ExecutionGate{NativeAllowed,WebRequired,RemoteApprovalRequired,
 RuntimeRequired,PortRequired}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LaunchError{NoConsent,UntrustedNative,UnsupportedRuntime}
/// Deterministic route planner: optional assistant recommendations cannot
/// bypass explicit consent, a signed-app gate or a missing runtime.
pub fn launch_gate(route:Availability,consented:bool,
 trusted_native:bool)->Result<ExecutionGate,LaunchError>{
 if !consented{return Err(LaunchError::NoConsent);}
 Ok(match route{
  Availability::Native=>{
   if !trusted_native{return Err(LaunchError::UntrustedNative);}
   ExecutionGate::NativeAllowed
  },
  Availability::RuntimeMissing=>ExecutionGate::RuntimeRequired,
  Availability::Web=>ExecutionGate::WebRequired,
  Availability::AuthorizedRemote=>ExecutionGate::RemoteApprovalRequired,
  Availability::PortRequired=>ExecutionGate::PortRequired,
 })
}
/// Only a verified native route is eligible for a local direct launch.
pub fn may_execute_locally(route:ExecutionGate)->bool{
 matches!(route,ExecutionGate::NativeAllowed)
}
#[cfg(test)]
mod orbit_r005_compat_tests{
 use super::*;
 #[test]fn native_batch_is_atomic_against_unverified_input(){
  let first=Requested{format:Format::Native,verified:true,bytes:b"",mac:None};
  let bad=Requested{format:Format::Native,verified:false,bytes:b"",mac:None};
  let mut out=[Availability::RuntimeMissing;2];
  assert_eq!(classify_batch(&[first,bad],62,&mut out),Err(Error::Unverified));
  assert_eq!(out,[Availability::RuntimeMissing;2]);
  assert_eq!(classify_batch(&[Requested{format:Format::Native,verified:true,bytes:b"",mac:None}],62,&mut out),Ok(1));
  assert_eq!(out[0],Availability::Native);
 }
 #[test]fn unsupported_platforms_never_convert_into_native_execution(){
  for route in [Availability::RuntimeMissing,Availability::Web,
   Availability::AuthorizedRemote,Availability::PortRequired]{
   let mode=launch_gate(route,true,true).unwrap();
   assert!(!may_execute_locally(mode));
  }
  assert_eq!(launch_gate(Availability::Native,false,true),Err(LaunchError::NoConsent));
  assert_eq!(launch_gate(Availability::Native,true,false),
   Err(LaunchError::UntrustedNative));
  assert!(may_execute_locally(launch_gate(Availability::Native,true,true).unwrap()));
 }
 #[test]fn short_output_does_not_expose_partly_classified_candidates(){
  let input=[Requested{format:Format::Native,verified:true,bytes:b"",mac:None}];
  assert_eq!(classify_batch(&input,62,&mut []),Err(Error::UnknownFormat));
 }
}


/// R017 workstation application launch chooser. Classification describes
/// compatibility requirements, never silently executes Windows/macOS/Linux.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum WorkstationLaunch{NativeReady,ExternalRuntimeNeeded,WebLink,RemoteOnly,RebuildRequired}
pub fn workstation_launch_plan(availability:Availability,
 consent:bool,signed_native:bool)->Result<WorkstationLaunch,LaunchError>{
 let gate=launch_gate(availability,consent,signed_native)?;
 Ok(match gate{
  ExecutionGate::Native=>WorkstationLaunch::NativeReady,
  _=>match availability{
   Availability::Native=>WorkstationLaunch::NativeReady,
   Availability::RuntimeMissing=>WorkstationLaunch::ExternalRuntimeNeeded,
   Availability::Web=>WorkstationLaunch::WebLink,
   Availability::AuthorizedRemote=>WorkstationLaunch::RemoteOnly,
   Availability::PortRequired=>WorkstationLaunch::RebuildRequired,
  },
 })
}
pub fn classify_workstation_batch(input:&[Requested<'_>],host_arch:u16,
 output:&mut[WorkstationLaunch],consent:bool)->Result<usize,Error>{
 if input.is_empty()||output.len()<input.len(){return Err(Error::UnknownFormat);}
 let mut stage=[WorkstationLaunch::RebuildRequired;16];
 if input.len()>stage.len(){return Err(Error::UnknownFormat);}
 for (i,request) in input.iter().enumerate(){
  let available=classify(Requested{format:request.format,
    verified:request.verified,bytes:request.bytes,mac:request.mac},host_arch)?;
  stage[i]=workstation_launch_plan(available,consent,request.verified)
    .map_err(|_|Error::Unverified)?;
 }
 output[..input.len()].copy_from_slice(&stage[..input.len()]);
 Ok(input.len())
}
#[cfg(test)]mod r017_workstation_compat_tests{
 use super::*;
 #[test]fn native_still_requires_signature_and_user_consent(){
  assert_eq!(workstation_launch_plan(Availability::Native,true,true),
    Ok(WorkstationLaunch::NativeReady));
  assert!(workstation_launch_plan(Availability::Native,false,true).is_err());
  assert!(workstation_launch_plan(Availability::Native,true,false).is_err());
 }
 #[test]fn third_party_runtime_is_never_treated_as_native(){
  let result=workstation_launch_plan(Availability::RuntimeMissing,true,true);
  assert_ne!(result,Ok(WorkstationLaunch::NativeReady));
 }
 #[test]fn batch_of_native_candidates_is_bounded_and_atomic(){
  let requests=[Requested{format:Format::Native,verified:true,bytes:b"",mac:None}];
  let mut out=[WorkstationLaunch::RebuildRequired;1];
  assert_eq!(classify_workstation_batch(&requests,62,&mut out,true),Ok(1));
  assert_eq!(out,[WorkstationLaunch::NativeReady]);
  assert!(classify_workstation_batch(&requests,62,&mut [],true).is_err());
 }
}
