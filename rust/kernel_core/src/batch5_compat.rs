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