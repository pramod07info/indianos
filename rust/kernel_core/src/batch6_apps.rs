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
  let mut elf=[0u8;64];elf[..4].copy_from_slice(b"\x7fELF");
  elf[4]=2;elf[5]=1;elf[18..20].copy_from_slice(&62u16.to_le_bytes());
  assert_eq!(classify(Requested{format:Format::ElfLinux,verified:true,
   bytes:&elf,mac:None},62),Ok(Availability::RuntimeMissing));
  assert_eq!(classify(Requested{format:Format::ElfLinux,verified:false,
   bytes:&elf,mac:None},62),Err(Error::Unverified));
 }
}