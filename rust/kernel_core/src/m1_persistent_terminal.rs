//! M1 (first integration slice): secure Terminal/Files operations on a
//! mounted, checksummed copy-on-write *block-backed* hierarchical VFS.
//! Unlike Batch 7's RAM store, files survive a device remount. This layer
//! never formats a device, never probes hardware, and never trusts raw PID
//! arguments from unprivileged callers. Kernel syscall ingress must supply
//! authenticated process identity, user buffers and monotonic tick values.
use crate::block::BlockDevice;
use crate::hier_vfs::{HierVfs,MAX_TRANSFER};
use crate::durablefs::FsError;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
use crate::vfs_path::normalize_absolute;
use crate::fs_write_guard::{WriteGuard,Error as GuardError};

pub const MAX_COMMAND:usize=192;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Io(FsError),OutputTooSmall,Unknown}
fn path<'a>(input:&str,buffer:&'a mut [u8;64])->Result<&'a str,Error>{
 let n=normalize_absolute(input,buffer).map_err(|_|Error::Invalid)?;
 if n<2{return Err(Error::Invalid);}
 core::str::from_utf8(&buffer[..n]).map_err(|_|Error::Invalid)
}
/// S1.1: protect reserved system namespaces at the M1 Terminal mutation
/// boundary too. This entrypoint bypasses the S0 FileHandles table, so it
/// must independently validate both source and destination before writes.
/// This is not yet a universal VFS/raw-block authorization layer.
fn writable_path(p:&str)->Result<(),Error>{
 match WriteGuard::allow_path(p){
  Ok(())=>Ok(()),
  Err(GuardError::InvalidPath)=>Err(Error::Invalid),
  Err(GuardError::Protected|GuardError::RateLimited)=>Err(Error::Denied),
 }
}
fn publish(data:&[u8],out:&mut[u8])->Result<usize,Error>{
 if data.len()>out.len(){return Err(Error::OutputTooSmall);}
 out[..data.len()].copy_from_slice(data);Ok(data.len())
}
pub fn command<D:BlockDevice,const N:usize>(
 fs:&mut HierVfs<D>,auth:&Access<N>,caller:Identity,now:u64,
 input:&[u8],out:&mut[u8])->Result<usize,Error>{
 if input.is_empty()||input.len()>MAX_COMMAND||input.iter().any(|c|
  *c!=b' '&&!c.is_ascii_graphic()){return Err(Error::Invalid);}
 let line=core::str::from_utf8(input).map_err(|_|Error::Invalid)?.trim();
 if line=="help"{return publish(b"pwd ls /dir cat /path write /path text append /path text mkdir /dir mv /old /new cp /src /dst rm /path",out);}
 if line=="pwd"{return publish(b"/",out);}
 let (verb,raw)=line.split_once(' ').ok_or(Error::Unknown)?;
 let mut name=[0u8;64];
 match verb{
  "ls"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let p=if raw=="/"{"/"}else{path(raw,&mut name)?};
   let mut buf=[0u8;MAX_TRANSFER];
   let n=fs.list_dir(p,&mut buf).map_err(Error::Io)?;
   publish(&buf[..n],out)
  }
  "cat"=>{
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let p=path(raw,&mut name)?;
   let mut buf=[0u8;MAX_TRANSFER];
   // Intentionally reject oversized files rather than disclose partial data.
   let n=fs.read(p,&mut buf).map_err(Error::Io)?;
   publish(&buf[..n],out)
  }
  "mkdir"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.mkdir(p).map_err(Error::Io)?;
   publish(b"created",out)
  }
  "write"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (raw_path,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   // Existing files are COW-overwritten; absent files are created. DurableFs
   // atomically publishes metadata; BlockDevice must flush each write.
   match fs.overwrite(p,data.as_bytes()){
    Ok(())=>{},
    Err(FsError::NotFound)=>fs.create(p,data.as_bytes()).map_err(Error::Io)?,
    Err(e)=>return Err(Error::Io(e)),
   };
   publish(b"saved",out)
  }
  "append"=>{
   // DurableFS commits a whole new checksummed image through write_at;
   // exact prior file content is verified before any new disk mutation.
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<8{return Err(Error::OutputTooSmall);}
   let (raw_path,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   let mut prior=[0u8;crate::durablefs::MAX_FILE_BYTES];
   let len=fs.read(p,&mut prior).map_err(Error::Io)?;
   if len.checked_add(data.len()).is_none_or(|n|
      n>crate::durablefs::MAX_FILE_BYTES){return Err(Error::Io(FsError::TooLarge));}
   fs.write_at(p,len as u64,data.as_bytes()).map_err(Error::Io)?;
   publish(b"appended",out)
  }
  "cp"=>{
   // A copy reads the source AND mutates the destination. Neither a
   // read-only nor a write-only grant may authorize this operation.
   auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<6{return Err(Error::OutputTooSmall);}
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let from=path(left,&mut name)?;
   let mut target=[0u8;64];
   let to=path(right,&mut target)?;
   if from==to{return Err(Error::Invalid);}
   writable_path(to)?;
   // Read the entire source before any destination mutation. In particular,
   // a missing/oversized source or a protected destination is fail-closed.
   let mut bytes=[0u8;MAX_TRANSFER];
   let n=fs.read(from,&mut bytes).map_err(Error::Io)?;
   match fs.overwrite(to,&bytes[..n]){
    Ok(())=>{},
    Err(FsError::NotFound)=>fs.create(to,&bytes[..n]).map_err(Error::Io)?,
    Err(e)=>return Err(Error::Io(e)),
   }
   publish(b"copied",out)
  }
  "rm"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.unlink(p).map_err(Error::Io)?;
   publish(b"removed",out)
  }
  "mv"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let from=path(left,&mut name)?;
   let mut target=[0u8;64];
   let to=path(right,&mut target)?;
   writable_path(from)?;
   writable_path(to)?;
   fs.rename_file(from,to).map_err(Error::Io)?;
   publish(b"moved",out)
  }
  _=>Err(Error::Unknown)
 }
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 const SECTORS:usize=100;
 #[derive(Clone)]
 struct Disk{data:[[u8;SECTOR_BYTES];SECTORS],fail_writes:bool}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{SECTORS as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   if self.fail_writes{return Err(BlockError::Io);}
   *self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*input;
   Ok(())
  }
 }
 fn blank()->Disk{Disk{data:[[0;SECTOR_BYTES];SECTORS],fail_writes:false}}
 fn user()->Identity{Identity{pid:3,generation:1}}
 fn grants()->Access<1>{let mut access=Access::new();
  access.grant(user(),FILE_READ|FILE_WRITE,500).unwrap();access}
 fn run(fs:&mut HierVfs<Disk>,access:&Access<1>,who:Identity,
   cmd:&[u8])->Result<(usize,[u8;MAX_TRANSFER]),Error>{
  let mut out=[0xaau8;MAX_TRANSFER];
  let size=command(fs,access,who,1,cmd,&mut out)?;
  Ok((size,out))
 }
 #[test]fn actual_directory_file_rename_and_reboot_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkdir /home").unwrap().0,7);
  assert_eq!(run(&mut fs,&access,user(),b"write /home/note hello").unwrap().0,5);
  let (n,b)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&b[..n],b"hello");
  let (n,b)=run(&mut fs,&access,user(),b"ls /home").unwrap();
  assert_eq!(&b[..n],b"f:note\n");
  run(&mut fs,&access,user(),b"mv /home/note /home/renamed").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"cat /home/renamed").unwrap();
  assert_eq!(&b[..n],b"hello");
  assert!(matches!(run(&mut fs,&access,user(),b"cat /home/note"),
   Err(Error::Io(FsError::NotFound))));
 }
 #[test]fn denied_mutation_and_failed_device_write_preserve_data(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/a original").unwrap();
  let wrong=Identity{generation:2,..user()};
  let before=fs.generation();
  assert_eq!(run(&mut fs,&access,wrong,b"write /home/a evil"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"write /../bad evil"),
   Err(Error::Invalid));
  assert_eq!(fs.generation(),before);
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"write /home/a evil"),
   Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut remount=HierVfs::mount(disk).unwrap();
  let (n,b)=run(&mut remount,&access,user(),b"cat /home/a").unwrap();
  assert_eq!(&b[..n],b"original");
 }
 #[test]fn read_output_must_be_large_enough_without_mutation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();fs.create("/x",b"hello").unwrap();
  let mut small=[0xccu8;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"cat /x",&mut small),
   Err(Error::OutputTooSmall));
  assert_eq!(small,[0xcc;2]);
 }
 #[test]fn security_m1_terminal_protects_namespaces_across_all_mutators(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  fs.mkdir("/system").unwrap();
  fs.create("/system/config",b"trusted").unwrap();
  fs.mkdir("/home").unwrap();
  fs.create("/home/note",b"personal").unwrap();
  let access=grants();
  let before=fs.generation();
  for cmd in [
   b"write /system/config corrupted".as_slice(),
   b"write //system/./config corrupt".as_slice(),
   b"mkdir /system/evil".as_slice(),
   b"mkdir /boot".as_slice(),
   b"mkdir /recovery".as_slice(),
   b"mv /system/config /home/stolen".as_slice(),
   b"mv /home/note /system/config".as_slice(),
   b"mv /home/note /recovery/file".as_slice(),
  ]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Denied),
              "{:?}",cmd);
   assert_eq!(fs.generation(),before,"denied command mutated the volume");
  }
  assert_eq!(run(&mut fs,&access,user(),
              b"write /system/../home/note corrupt"),
             Err(Error::Invalid));
  let (n,buf)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&buf[..n],b"trusted"); // read-only inspection remains allowed
  let (n,buf)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&buf[..n],b"personal");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,buf)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&buf[..n],b"trusted");
  let (n,buf)=run(&mut fs,&access,user(),b"cat /home/note").unwrap();
  assert_eq!(&buf[..n],b"personal");
 }
 #[test]fn terminal_removal_and_multi_write_survive_separate_remounts(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /docs").unwrap();
  run(&mut fs,&access,user(),b"write /docs/note first").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  run(&mut fs,&access,user(),b"write /docs/note second").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (count,bytes)=run(&mut fs,&access,user(),b"cat /docs/note").unwrap();
  assert_eq!(&bytes[..count],b"second");
  let (count,bytes)=run(&mut fs,&access,user(),b"rm /docs/note").unwrap();
  assert_eq!(&bytes[..count],b"removed");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cat /docs/note"),
    Err(Error::Io(FsError::NotFound)));
 }
 #[test]fn terminal_remove_denied_and_short_output_cannot_delete_data(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/system").unwrap();
  fs.create("/system/config",b"trusted").unwrap();
  fs.mkdir("/docs").unwrap();fs.create("/docs/file",b"keep").unwrap();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"rm /system/config"),Err(Error::Denied));
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&access,stale,b"rm /docs/file"),Err(Error::Denied));
  let mut short=[0xf0;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"rm /docs/file",&mut short),
    Err(Error::OutputTooSmall));
  assert_eq!(short,[0xf0;2]);
  assert_eq!(fs.generation(),generation);
  let (n,out)=run(&mut fs,&access,user(),b"cat /docs/file").unwrap();
  assert_eq!(&out[..n],b"keep");
 }
 #[test]fn security_m1_namespace_guard_does_not_block_unrelated_names(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkdir /systematic").unwrap().0,7);
  assert_eq!(run(&mut fs,&access,user(),
                b"write /systematic/log okay").unwrap().0,5);
  let (n,b)=run(&mut fs,&access,user(),b"cat /systematic/log").unwrap();
  assert_eq!(&b[..n],b"okay");
 }

 #[test]fn terminal_append_is_committed_across_multiple_remounts(){
  let access=grants();
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/log first").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,reply)=run(&mut fs,&access,user(),b"append /home/log +second").unwrap();
  assert_eq!(&reply[..n],b"appended");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  run(&mut fs,&access,user(),b"append /home/log +third").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,contents)=run(&mut fs,&access,user(),b"cat /home/log").unwrap();
  assert_eq!(&contents[..n],b"first+second+third");
 }
 #[test]fn terminal_append_denial_and_torn_write_preserve_last_commit(){
  let access=grants();
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  run(&mut fs,&access,user(),b"mkdir /docs").unwrap();
  run(&mut fs,&access,user(),b"write /docs/notes stable").unwrap();
  let before=fs.generation();
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&access,stale,b"append /docs/notes evil"),
    Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"append /boot/kernel evil"),
    Err(Error::Denied));
  let mut too_small=[0x5a;7];
  assert_eq!(command(&mut fs,&access,user(),1,b"append /docs/notes evil",
    &mut too_small),Err(Error::OutputTooSmall));
  assert_eq!(too_small,[0x5a;7]);
  assert_eq!(fs.generation(),before);
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"append /docs/notes unsafe"),
    Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut fs=HierVfs::mount(disk).unwrap();
  let (n,content)=run(&mut fs,&access,user(),b"cat /docs/notes").unwrap();
  assert_eq!(&content[..n],b"stable");
 }
 #[test]fn terminal_copy_is_durable_and_requires_both_rights(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();
  fs.create("/home/source",b"durable source").unwrap();
  fs.create("/home/target",b"old target").unwrap();
  let mut read_only=Access::<1>::new();
  read_only.grant(user(),FILE_READ,500).unwrap();
  let mut write_only=Access::<1>::new();
  write_only.grant(user(),FILE_WRITE,500).unwrap();
  let before=fs.generation();
  assert_eq!(run(&mut fs,&read_only,user(),b"cp /home/source /home/target"),
             Err(Error::Denied));
  assert_eq!(run(&mut fs,&write_only,user(),b"cp /home/source /home/target"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),before);
  let (n,result)=run(&mut fs,&access,user(),b"cp /home/source /home/target").unwrap();
  assert_eq!(&result[..n],b"copied");
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"durable source");
  run(&mut fs,&access,user(),b"cp /home/source /home/second").unwrap();
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/second").unwrap();
  assert_eq!(&data[..n],b"durable source");
 }
 #[test]fn terminal_copy_rejected_paths_and_small_output_do_not_mutate(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();fs.mkdir("/system").unwrap();
  fs.create("/home/source",b"good").unwrap();
  fs.create("/home/target",b"keep").unwrap();
  fs.create("/system/config",b"protected").unwrap();
  let before=fs.generation();
  for cmd in [
   b"cp /home/source /system/config".as_slice(),
   b"cp /system/config /boot/config".as_slice(),
  ]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Denied));
  }
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/source /home/source"),
             Err(Error::Invalid));
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/missing /home/target"),
             Err(Error::Io(FsError::NotFound)));
  let mut small=[0xcc;5];
  assert_eq!(command(&mut fs,&access,user(),1,
             b"cp /home/source /home/target",&mut small),
             Err(Error::OutputTooSmall));
  assert_eq!(small,[0xcc;5]);
  assert_eq!(fs.generation(),before);
  fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"keep");
  let (n,data)=run(&mut fs,&access,user(),b"cat /system/config").unwrap();
  assert_eq!(&data[..n],b"protected");
 }
 #[test]fn terminal_copy_failed_disk_write_keeps_last_committed_target(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.mkdir("/home").unwrap();
  fs.create("/home/source",b"replacement").unwrap();
  fs.create("/home/target",b"committed").unwrap();
  let mut disk=fs.into_device();disk.fail_writes=true;
  let mut fs=HierVfs::mount(disk).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cp /home/source /home/target"),
             Err(Error::Io(FsError::Io)));
  let mut disk=fs.into_device();disk.fail_writes=false;
  let mut fs=HierVfs::mount(disk).unwrap();
  let (n,data)=run(&mut fs,&access,user(),b"cat /home/target").unwrap();
  assert_eq!(&data[..n],b"committed");
 }

}
