//! Explicit kernel-only install consent ledger: a permission approval is
//! bound to one package version/digest, process PID+generation and expiry.
//! A saved signature authorizes publisher identity but NEVER implies that a
//! human consented to requested FILE/NETWORK/DRIVER permissions.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL,ALL};
use crate::m2_inventory::Record;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Expired,Capacity,Duplicate}
/// Distinguish authorization failures from real block-backed filesystem I/O.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FileError{Authorization(Error),Storage(crate::durablefs::FsError)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Approval{
 process:Identity,app:u32,version:u64,digest:[u8;32],
 approved_rights:u32,expires:u64,
}
pub struct Ledger<const P:usize>{records:[Option<Approval>;P]}
impl<const P:usize> Ledger<P>{
 pub const fn new()->Self{Self{records:[None;P]}}
 /// The trusted supervisor supplies BOTH the real process identity AND
 /// the explicit administrator decision; this never displays a GUI itself.
 pub fn approve<const N:usize>(
  &mut self,auth:&Access<N>,administrator:Identity,now:u64,
  process:Identity,app:Record,approved_rights:u32,expires:u64,
 )->Result<(),Error>{
  auth.require(administrator,INSTALL,now).map_err(|_|Error::Denied)?;
  if process.pid==0||process.generation==0||app.id==0||app.version==0||
     app.digest==[0;32]||approved_rights&!ALL!=0||
     app.rights&!approved_rights!=0||expires<=now{
    return Err(Error::Invalid);
  }
  // Never silently transfer old permissions to a reused PID.
  if self.records.iter().flatten().any(|a|a.process.pid==process.pid){
   return Err(Error::Duplicate);
  }
  let slot=self.records.iter_mut().find(|x|x.is_none())
   .ok_or(Error::Capacity)?;
  *slot=Some(Approval{process,app:app.id,version:app.version,
    digest:app.digest,approved_rights,expires});
  Ok(())
 }
 /// Do not use package name alone: stale generations, upgraded binaries,
 /// changed rights or revoked grants must all fail closed before ring3.
 pub fn require(&self,process:Identity,app:Record,now:u64)->Result<(),Error>{
  let approval=self.records.iter().flatten().find(|a|a.process.pid==process.pid)
    .ok_or(Error::Denied)?;
  if approval.process!=process||approval.app!=app.id||
     approval.version!=app.version||approval.digest!=app.digest||
     (app.rights&!approval.approved_rights)!=0{
   return Err(Error::Denied);
  }
  if now>=approval.expires{return Err(Error::Expired);}
  Ok(())
 }
 /// Kernel-side signed-application filesystem authorization preflight.
 /// The trusted syscall ingress must obtain the PID and generation from
 /// its scheduler, never from untrusted user arguments. All modifying VFS,
 /// mmap and raw-storage entrypoints still need independent integration.
 pub fn require_file_access(&self,process:Identity,app:Record,
   now:u64,path:&str,required:u32)->Result<(),Error>{
  self.require(process,app,now)?;
  if required!=crate::batch6_security::FILE_READ&&
     required!=crate::batch6_security::FILE_WRITE{
   return Err(Error::Denied);
  }
  if app.rights&required!=required{return Err(Error::Denied);}
  // Even a signed installed app may not directly access firmware/system keys,
  // recovery images or snapshot stores through this untrusted-file surface.
  crate::fs_write_guard::WriteGuard::allow_path(path)
    .map_err(|_|Error::Denied)?;
  Ok(())
 }
 /// Authorized data read through a real mounted persistent filesystem.
 /// The trusted syscall broker MUST supply the scheduler-derived process
 /// identity and the verified installed Record, never user-provided values.
 /// This wrapper is not yet a universal syscall or raw-block access hook.
 pub fn read_authorized<D:crate::block::BlockDevice>(
  &self,fs:&mut crate::hier_vfs::HierVfs<D>,process:Identity,
  app:Record,now:u64,path:&str,out:&mut[u8]
 )->Result<usize,FileError>{
  self.require_file_access(process,app,now,path,
    crate::batch6_security::FILE_READ).map_err(FileError::Authorization)?;
  fs.read(path,out).map_err(FileError::Storage)
 }
 /// All destination checks run before any copy-on-write transaction.
 /// Missing files are created only after the same authorization preflight.
 pub fn write_authorized<D:crate::block::BlockDevice>(
  &self,fs:&mut crate::hier_vfs::HierVfs<D>,
  guard:&mut crate::fs_write_guard::WriteGuard,process:Identity,
  app:Record,now:u64,path:&str,data:&[u8]
 )->Result<(),FileError>{
  self.require_file_access(process,app,now,path,
    crate::batch6_security::FILE_WRITE).map_err(FileError::Authorization)?;
  // Reuse the existing S0 per-service ransomware/write budget rather than
  // introducing an unmetered alternate write entry point.
  guard.preflight(path,data.len())
    .map_err(|_|FileError::Authorization(Error::Denied))?;
  let result=match fs.overwrite(path,data){
   Ok(())=>Ok(()),
   Err(crate::durablefs::FsError::NotFound)=>
    fs.create(path,data).map_err(FileError::Storage),
   Err(e)=>Err(FileError::Storage(e)),
  };
  if result.is_ok(){guard.completed(data.len());}
  result
 }
 pub fn revoke(&mut self,process:Identity)->Result<(),Error>{
  let Some(slot)=self.records.iter_mut().find(|x|
    x.is_some_and(|a|a.process==process))else{return Err(Error::Denied)};
  *slot=None;Ok(())
 }
 /// The real kernel reaper must call this BEFORE reusing a process PID.
 pub fn retire(&mut self,process:Identity){let _=self.revoke(process);}
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::batch6_security::{FILE_READ,FILE_WRITE,SOCKET};
 fn actor()->Identity{Identity{pid:2,generation:7}}
 fn process()->Identity{Identity{pid:9,generation:1}}
 fn app()->Record{Record{id:4,version:2,rights:FILE_READ,
  digest:[3;32],deps:[0;4],minimums:[0;4]}}
 fn auth()->Access<1>{let mut a=Access::new();
  a.grant(actor(),INSTALL,100).unwrap();a
 }
 #[test]fn m2_consent_binds_pid_generation_version_digest_rights_and_time(){
  let mut ledger=Ledger::<1>::new();
  assert_eq!(ledger.require(process(),app(),1),Err(Error::Denied));
  assert_eq!(ledger.approve(&auth(),actor(),1,process(),app(),0,20),
   Err(Error::Invalid));
  ledger.approve(&auth(),actor(),1,process(),app(),FILE_READ,20).unwrap();
  assert_eq!(ledger.require(process(),app(),1),Ok(()));
  assert_eq!(ledger.require(Identity{generation:2,..process()},app(),1),Err(Error::Denied));
  assert_eq!(ledger.require(process(),Record{version:3,..app()},1),Err(Error::Denied));
  assert_eq!(ledger.require(process(),Record{digest:[4;32],..app()},1),Err(Error::Denied));
  assert_eq!(ledger.require(process(),Record{rights:FILE_READ|FILE_WRITE,..app()},1),Err(Error::Denied));
  assert_eq!(ledger.require(process(),app(),20),Err(Error::Expired));
  assert_eq!(ledger.approve(&auth(),actor(),1,Identity{generation:2,..process()},
    app(),FILE_READ,30),Err(Error::Duplicate));
  ledger.retire(process());
  ledger.approve(&auth(),actor(),1,Identity{generation:2,..process()},
    app(),FILE_READ,30).unwrap();
  assert_eq!(ledger.require(process(),app(),1),Err(Error::Denied));
  assert_eq!(ledger.revoke(Identity{generation:2,..process()}),Ok(()));
  assert_eq!(ledger.require(Identity{generation:2,..process()},app(),1),Err(Error::Denied));
 }
 #[test]fn m2_consent_file_access_requires_signed_image_and_exact_live_pid(){
  let app=Record{rights:FILE_READ|FILE_WRITE,..app()};
  let mut guard=crate::fs_write_guard::WriteGuard::new();
  let mut ledger=Ledger::<1>::new();
  ledger.approve(&auth(),actor(),1,process(),app,
    FILE_READ|FILE_WRITE,30).unwrap();
  for right in [FILE_READ,FILE_WRITE]{
   assert_eq!(ledger.require_file_access(process(),app,2,
     "/home/notes",right),Ok(()));
  }
  assert_eq!(ledger.require_file_access(process(),app,2,
    "/keys/private",FILE_READ),Err(Error::Denied));
  assert_eq!(ledger.require_file_access(process(),app,2,
    "/boot/kernel",FILE_WRITE),Err(Error::Denied));
  assert_eq!(ledger.require_file_access(process(),app,2,
    "/home/../boot/kernel",FILE_READ),Err(Error::Denied));
  assert_eq!(ledger.require_file_access(process(),app,2,
    "/home/notes",INSTALL),Err(Error::Denied));
  assert_eq!(ledger.require_file_access(Identity{generation:2,..process()},
    app,2,"/home/notes",FILE_READ),Err(Error::Denied));
  assert_eq!(ledger.require_file_access(process(),
    Record{digest:[5;32],..app},2,"/home/notes",FILE_READ),
    Err(Error::Denied));
  assert_eq!(ledger.require_file_access(process(),app,30,
    "/home/notes",FILE_READ),Err(Error::Expired));
  ledger.retire(process());
  assert_eq!(ledger.require_file_access(process(),app,2,
    "/home/notes",FILE_READ),Err(Error::Denied));
 }
 #[test]fn m2_consent_rejects_unauthenticated_or_stale_administrators(){
  let mut ledger=Ledger::<1>::new();
  assert_eq!(ledger.approve(&auth(),Identity{generation:8,..actor()},1,
    process(),app(),FILE_READ,20),Err(Error::Denied));
  assert_eq!(ledger.approve(&auth(),actor(),100,
    process(),app(),FILE_READ,120),Err(Error::Denied));
  let mut no_scope=app();no_scope.rights=FILE_READ|SOCKET;
  assert_eq!(ledger.approve(&auth(),actor(),1,
    process(),no_scope,FILE_READ,20),Err(Error::Invalid));
  ledger.approve(&auth(),actor(),1,process(),app(),FILE_READ,20).unwrap();
  assert_eq!(ledger.approve(&auth(),actor(),1,
    Identity{pid:10,generation:1},app(),FILE_READ,20),Err(Error::Capacity));
 }
 #[test]fn m2_consented_filesystem_io_enforces_rights_and_remounts(){
  use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
  use crate::hier_vfs::HierVfs;
  const SECTORS:usize=100;
  struct Disk{data:[[u8;SECTOR_BYTES];SECTORS]}
  impl BlockDevice for Disk{
   fn sector_count(&self)->u64{SECTORS as u64}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
   }
   fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*from;Ok(())
   }
  }
  let mut fs=HierVfs::format_blank(Disk{
   data:[[0;SECTOR_BYTES];SECTORS]
  }).unwrap();
  fs.mkdir("/home").unwrap();fs.mkdir("/system").unwrap();
  fs.create("/system/config",b"trusted").unwrap();
  let app=Record{rights:FILE_READ|FILE_WRITE,..app()};
  let mut ledger=Ledger::<1>::new();
  ledger.approve(&auth(),actor(),1,process(),app,
    FILE_READ|FILE_WRITE,30).unwrap();
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app,2,
    "/home/note",b"durable"),Ok(()));
  let mut output=[0u8;16];
  assert_eq!(ledger.read_authorized(&mut fs,process(),app,2,
    "/home/note",&mut output),Ok(7));
  assert_eq!(&output[..7],b"durable");
  let generation=fs.generation();
  assert_eq!(guard.statistics().completed_writes,1);
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app,2,
    "/home/note",b""),
    Err(FileError::Authorization(Error::Denied)));
  assert_eq!(fs.generation(),generation);
  let stale=Identity{generation:2,..process()};
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,stale,app,2,
    "/home/note",b"attack"),
    Err(FileError::Authorization(Error::Denied)));
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app,2,
    "/system/config",b"attack"),
    Err(FileError::Authorization(Error::Denied)));
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app,2,
    "/home/../system/config",b"attack"),
    Err(FileError::Authorization(Error::Denied)));
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app,30,
    "/home/note",b"expired"),
    Err(FileError::Authorization(Error::Expired)));
  assert_eq!(fs.generation(),generation);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(ledger.read_authorized(&mut fs,process(),app,2,
    "/home/note",&mut output),Ok(7));
  assert_eq!(&output[..7],b"durable");
  let mut protected=[0u8;16];
  assert_eq!(fs.read("/system/config",&mut protected),Ok(7));
  assert_eq!(&protected[..7],b"trusted");
  ledger.retire(process());
  assert_eq!(ledger.read_authorized(&mut fs,process(),app,2,
    "/home/note",&mut output),
    Err(FileError::Authorization(Error::Denied)));
 }
 #[test]fn m2_consented_filesystem_read_only_grant_cannot_write(){
  use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
  use crate::hier_vfs::HierVfs;
  struct Disk{data:[[u8;SECTOR_BYTES];100]}
  impl BlockDevice for Disk{
   fn sector_count(&self)->u64{100}
   fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
   }
   fn write_sector(&mut self,lba:u64,from:&[u8;SECTOR_BYTES])
    ->Result<(),BlockError>{
    *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*from;Ok(())
   }
  }
  let mut fs=HierVfs::format_blank(Disk{data:[[0;SECTOR_BYTES];100]}).unwrap();
  fs.mkdir("/home").unwrap();fs.create("/home/a",b"original").unwrap();
  let mut ledger=Ledger::<1>::new();
  let mut guard=crate::fs_write_guard::WriteGuard::new();
  ledger.approve(&auth(),actor(),1,process(),app(),FILE_READ,20).unwrap();
  let generation=fs.generation();
  assert_eq!(ledger.write_authorized(&mut fs,&mut guard,process(),app(),2,
    "/home/a",b"changed"),
    Err(FileError::Authorization(Error::Denied)));
  assert_eq!(fs.generation(),generation);
  let mut buf=[0u8;16];
  assert_eq!(ledger.read_authorized(&mut fs,process(),app(),2,
    "/home/a",&mut buf),Ok(8));
  assert_eq!(&buf[..8],b"original");
 }

}
