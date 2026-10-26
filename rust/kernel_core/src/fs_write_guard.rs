//! S0 defense-in-depth for the existing R031 per-process file-handle write ABI.
//!
//! This is NOT a complete ransomware detector or a global VFS ACL.
//! It defends the current FileHandles::open/write syscall surface only.
//! Direct privileged VFS/ATA callers remain separate S1 migration work.
//!
//! All checks are allocation-free and fail closed. A process cannot
//! evade the monotonic write budget simply by closing/reopening a handle.
//! Budget resets if the FileHandles kernel service itself is recreated;
//! this is deliberately NOT claimed to be durable ransomware protection.
use crate::vfs_path::normalize_absolute;

pub const MAX_WRITE_OPERATIONS:u32=32;
pub const MAX_WRITE_BYTES:u32=4096;
const PROTECTED:&[&[u8]]=&[
 b"/boot",b"/system",b"/security",b"/recovery",
 b"/snapshots",b"/keys",b"/firmware",
];
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error{InvalidPath,Protected,RateLimited}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Statistics{
 pub completed_writes:u32,pub changed_bytes:u32,pub denials:u32,
}
/// Kernel-owned service-scoped counter, never stored in a user process.
/// Caller must serialize access; existing file handle table is single-core.
pub struct WriteGuard{
 completed:u32,bytes:u32,denials:u32,
}
impl WriteGuard{
 pub const fn new()->Self{Self{completed:0,bytes:0,denials:0}}
 pub fn statistics(&self)->Statistics{Statistics{
  completed_writes:self.completed,changed_bytes:self.bytes,
  denials:self.denials
 }}
 pub fn deny(&mut self){
  self.denials=self.denials.saturating_add(1);
 }
 /// Protect exact trusted namespaces and all descendants, but avoid
 /// matching innocent names such as /systematic or /bootcamp.
 pub fn allow_path(path:&str)->Result<(),Error>{
  let mut canonical=[0u8;64];
  let n=normalize_absolute(path,&mut canonical).map_err(|_|Error::InvalidPath)?;
  let name=&canonical[..n];
  if PROTECTED.iter().any(|p|{
   name==*p||(name.starts_with(p)&&name.get(p.len())==Some(&b'/'))
  }){
   return Err(Error::Protected);
  }
  Ok(())
 }
 /// Before any mutation: stop writes to sensitive namespaces and
 /// large/repeated write attempts through this specific file ABI.
 pub fn preflight(&mut self,path:&str,length:usize)->Result<(),Error>{
  let error=(||{
   Self::allow_path(path)?;
   if length==0||length>u32::MAX as usize{
    return Err(Error::RateLimited);
   }
   let next=self.completed.checked_add(1).ok_or(Error::RateLimited)?;
   let bytes=self.bytes.checked_add(length as u32).ok_or(Error::RateLimited)?;
   if next>MAX_WRITE_OPERATIONS||bytes>MAX_WRITE_BYTES{
    return Err(Error::RateLimited);
   }
   Ok(())
  })();
  if error.is_err(){self.deny();}
  error
 }
 /// Called ONLY after successful durable filesystem write.
 /// The file-handle service verifies the actual count never exceeds
 /// the number of bytes admitted by preflight.
 pub fn completed(&mut self,written:usize){
  self.completed=self.completed.saturating_add(1);
  self.bytes=self.bytes.saturating_add(written as u32);
 }
}
#[cfg(test)]
mod security_tests{
 use super::*;
 #[test]fn security_sensitive_directories_are_write_protected(){
  for path in [
   "/system","/system/driver","/boot/init","/security/keys",
   "/recovery/snapshot","/snapshots/history","/keys/public",
   "/firmware/update","//system/./driver",
  ]{
   assert_eq!(WriteGuard::allow_path(path),Err(Error::Protected),"{path}");
  }
  for path in ["/etc/note","/shared/notes","/systematic/file",
   "/bootcamp/notes","/documents/security","/user/boot"]{
   assert_eq!(WriteGuard::allow_path(path),Ok(()),"{path}");
  }
 }
 #[test]fn security_traversal_nul_and_invalid_names_fail_closed(){
  for path in ["/system/../etc/note","/etc/../../system",
   "system/file","/etc/\0evil",""]{
   assert_eq!(WriteGuard::allow_path(path),Err(Error::InvalidPath));
  }
 }
 #[test]fn security_write_budget_cannot_reset_by_new_open_or_short_operation(){
  let mut g=WriteGuard::new();
  for _ in 0..MAX_WRITE_OPERATIONS{
   g.preflight("/shared/notes",1).unwrap();
   g.completed(1);
  }
  assert_eq!(g.preflight("/shared/notes",1),Err(Error::RateLimited));
  assert_eq!(g.statistics(),Statistics{
   completed_writes:MAX_WRITE_OPERATIONS,
   changed_bytes:MAX_WRITE_OPERATIONS,denials:1,
  });
  assert_eq!(g.preflight("/boot/startup",1),Err(Error::Protected));
  assert_eq!(g.statistics().denials,2);
 }
 #[test]fn security_failed_write_is_not_charged_and_byte_budget_enforced(){
  let mut g=WriteGuard::new();
  assert_eq!(g.preflight("/etc/note",MAX_WRITE_BYTES as usize+1),
   Err(Error::RateLimited));
  assert_eq!(g.statistics().changed_bytes,0);
  assert_eq!(g.preflight("/etc/note",128),Ok(()));
  assert_eq!(g.statistics().completed_writes,0);
  g.completed(128);
  assert_eq!(g.statistics().changed_bytes,128);
  assert_eq!(g.preflight("/shared/docs",3970),Err(Error::RateLimited));
 }
}
