//! Batch 5 tasks 16–19: route binary metadata into explicit compatibility
//! decisions. No cross-platform execution is possible from these validators.
use crate::batch4_windows_pe;
use crate::batch4_android;
use crate::batch4_macos_policy;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Route{Native,NeedsLinuxRuntime,NeedsWin32Runtime,
 NeedsAndroidRuntime,PortableRebuild,Web,AuthorizedRemote}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Unverified,Malformed,Unsupported,Unlicensed}
pub fn native(signed:bool,arch:u16,host:u16)->Result<Route,Error>{
 if !signed{return Err(Error::Unverified);}
 if host==0||arch!=host{return Err(Error::Unsupported);}
 Ok(Route::Native)
}
pub fn linux_x86_64_elf(signed:bool,data:&[u8])->Result<Route,Error>{
 if !signed{return Err(Error::Unverified);}
 if data.len()<64||data[..4]!=[0x7f,b'E',b'L',b'F']||
  data[4]!=2||data[5]!=1{return Err(Error::Malformed);}
 if data[18..20]!=62u16.to_le_bytes(){return Err(Error::Unsupported);}
 Ok(Route::NeedsLinuxRuntime)
}
pub fn windows_x86_64(signed:bool,data:&[u8])->Result<Route,Error>{
 if !signed{return Err(Error::Unverified);}
 batch4_windows_pe::parse(data).map_err(|_|Error::Malformed)?;
 Ok(Route::NeedsWin32Runtime)
}
pub fn android_apk(signed:bool,data:&[u8])->Result<Route,Error>{
 if !signed{return Err(Error::Unverified);}
 batch4_android::first_name(data).map_err(|_|Error::Malformed)?;
 Ok(Route::NeedsAndroidRuntime)
}
pub fn macos(app:batch4_macos_policy::App)->Result<Route,Error>{
 match batch4_macos_policy::choose(app){
  Ok(batch4_macos_policy::Route::PortableRebuild)=>Ok(Route::PortableRebuild),
  Ok(batch4_macos_policy::Route::Web)=>Ok(Route::Web),
  Ok(batch4_macos_policy::Route::AuthorizedRemote)=>Ok(Route::AuthorizedRemote),
  Err(batch4_macos_policy::Error::Unauthorized)=>Err(Error::Unlicensed),
  Err(_)=>Err(Error::Unsupported),
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn linux_binary_is_not_mistaken_for_running_program(){
  let mut elf=[0u8;64];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  assert_eq!(linux_x86_64_elf(true,&elf),Ok(Route::NeedsLinuxRuntime));
  assert_eq!(linux_x86_64_elf(false,&elf),Err(Error::Unverified));
 }
 #[test]fn missing_mac_entitlement_denied(){
  assert_eq!(macos(batch4_macos_policy::App{
   source_portable:false,web_app:false,remote_license:false,
   authorized_host:false,proprietary_frameworks:true}),Err(Error::Unlicensed));
 }
}

/// Availability is supplied only by the trusted kernel runtime registry;
/// these bits are NOT read from untrusted application manifests.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TrustedRuntimes{
 pub linux:bool,pub win32:bool,pub android:bool,pub web:bool,
 pub authorized_remote:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RuntimeError{Unsigned,NoConsent,NoGrant,Unavailable}
/// Route admission is distinct from classification. An unsigned executable,
/// expired/recycled process, absent compatible runtime or missing admin
/// consent cannot transition to execution. No third-party code is launched.
pub fn authorize_runtime<const N:usize>(
 route:Route,verified:bool,consent:bool,
 owner:crate::batch5_session::Identity,now:u64,
 auth:&crate::batch6_security::Access<N>,runtimes:TrustedRuntimes,
)->Result<Route,RuntimeError>{
 if !verified{return Err(RuntimeError::Unsigned);}
 if !consent{return Err(RuntimeError::NoConsent);}
 let required=if route==Route::AuthorizedRemote{
  crate::batch6_security::VIEW|crate::batch6_security::SOCKET
 }else{crate::batch6_security::VIEW};
 auth.require(owner,required,now).map_err(|_|RuntimeError::NoGrant)?;
 let available=match route{
  Route::Native|Route::PortableRebuild=>true,
  Route::NeedsLinuxRuntime=>runtimes.linux,
  Route::NeedsWin32Runtime=>runtimes.win32,
  Route::NeedsAndroidRuntime=>runtimes.android,
  Route::Web=>runtimes.web,
  Route::AuthorizedRemote=>runtimes.authorized_remote,
 };
 if !available{return Err(RuntimeError::Unavailable);}
 Ok(route)
}
#[cfg(test)]
mod guarded_runtime_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,SOCKET,VIEW};
 fn principal()->Identity{Identity{pid:22,generation:5}}
 fn auth()->Access<1>{
  let mut a=Access::new();
  a.grant(principal(),VIEW|SOCKET,100).unwrap();a
 }
 fn all()->TrustedRuntimes{TrustedRuntimes{
  linux:true,win32:true,android:true,web:true,authorized_remote:true,
 }}
 #[test]fn classified_foreign_format_requires_actual_runtime(){
  let a=auth();
  let absent=TrustedRuntimes{linux:false,win32:false,android:false,
    web:false,authorized_remote:false};
  for route in [Route::NeedsLinuxRuntime,Route::NeedsWin32Runtime,
                Route::NeedsAndroidRuntime,Route::Web,Route::AuthorizedRemote]{
   assert_eq!(authorize_runtime(route,true,true,principal(),2,&a,absent),
              Err(RuntimeError::Unavailable));
   assert_eq!(authorize_runtime(route,true,true,principal(),2,&a,all()),
              Ok(route));
  }
 }
 #[test]fn unsigned_and_unconsented_executables_never_run(){
  let a=auth();
  assert_eq!(authorize_runtime(Route::NeedsLinuxRuntime,false,true,
      principal(),2,&a,all()),Err(RuntimeError::Unsigned));
  assert_eq!(authorize_runtime(Route::Native,true,false,
      principal(),2,&a,all()),Err(RuntimeError::NoConsent));
 }
 #[test]fn reused_pid_and_expired_grants_are_not_inherited(){
  let a=auth();
  let recycled=Identity{generation:6,..principal()};
  assert_eq!(authorize_runtime(Route::Native,true,true,recycled,2,&a,all()),
             Err(RuntimeError::NoGrant));
  assert_eq!(authorize_runtime(Route::Native,true,true,principal(),100,&a,all()),
             Err(RuntimeError::NoGrant));
 }
 #[test]fn remote_compatibility_additionally_requires_socket_rights(){
  let mut a=Access::<1>::new();a.grant(principal(),VIEW,100).unwrap();
  assert_eq!(authorize_runtime(Route::AuthorizedRemote,true,true,
      principal(),2,&a,all()),Err(RuntimeError::NoGrant));
  assert_eq!(authorize_runtime(Route::NeedsLinuxRuntime,true,true,
      principal(),2,&a,all()),Ok(Route::NeedsLinuxRuntime));
 }
}
