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
 if line=="help"{return publish(b"pwd ls /dir cat /path write /path text append /path text mkdir /dir mv /old /new cp /src /dst rm /path mkfile /path truncate /path head /path count rmdir /dir mvdir /old /new pread /file offset count pwrite /file@offset text",out);}
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
  "mkfile"=>{
   // Creation is explicit, never truncates an existing file. Check the
   // output and authorization before changing durable device metadata.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.create(p,&[]).map_err(Error::Io)?;
   publish(b"created",out)
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
  "truncate"=>{
   // Retains the existing file entry, empties only its data, and publishes
   // through the already mounted COW volume. Never auto-creates a path.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<9{return Err(Error::OutputTooSmall);}
   let p=path(raw,&mut name)?;
   writable_path(p)?;
   fs.overwrite(p,b"").map_err(Error::Io)?;
   publish(b"truncated",out)
  }
  "head"=>{
   // Read-only, bounded prefix output from a persisted file. A user can
   // request no more than the VFS transfer limit; malformed counts fail.
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let (raw_path,raw_count)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let count=raw_count.parse::<usize>().map_err(|_|Error::Invalid)?;
   if count==0||count>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   let mut bytes=[0u8;MAX_TRANSFER];
   let size=fs.read(p,&mut bytes).map_err(Error::Io)?;
   publish(&bytes[..core::cmp::min(count,size)],out)
  }
  "rmdir"=>{
   // Delete empty directories only, using the persistent VFS tree guard.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<7{return Err(Error::OutputTooSmall);}
   let dir=path(raw,&mut name)?;
   writable_path(dir)?;
   fs.rmdir(dir).map_err(Error::Io)?;
   publish(b"removed",out)
  }
  "mvdir"=>{
   // Move an entire directory subtree transactionally: verify both names
   // and deny protected namespaces before any COW metadata modification.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (src,dest)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let src=path(src,&mut name)?;
   let mut target=[0u8;64];
   let dest=path(dest,&mut target)?;
   writable_path(src)?;
   writable_path(dest)?;
   fs.rename_dir(src,dest).map_err(Error::Io)?;
   publish(b"moved",out)
  }
  "pread"=>{
   // Bounded random-access read with an authenticated process grant.
   auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
   let mut items=raw.split(' ');
   let raw_path=items.next().ok_or(Error::Invalid)?;
   let offset=items.next().ok_or(Error::Invalid)?
     .parse::<u64>().map_err(|_|Error::Invalid)?;
   let count=items.next().ok_or(Error::Invalid)?
     .parse::<usize>().map_err(|_|Error::Invalid)?;
   if items.next().is_some()||count==0||count>MAX_TRANSFER{
    return Err(Error::Invalid);
   }
   let p=path(raw_path,&mut name)?;
   let mut data=[0u8;MAX_TRANSFER];
   let n=fs.read_at(p,offset,&mut data[..count]).map_err(Error::Io)?;
   publish(&data[..n],out)
  }
  "pwrite"=>{
   // In-place bounded persistent update; never creates sparse holes or
   // bypasses the protected mount namespace. Output preflight precedes I/O.
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (where_at,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let (raw_path,offset)=where_at.split_once('@').ok_or(Error::Invalid)?;
   let offset=offset.parse::<u64>().map_err(|_|Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   writable_path(p)?;
   fs.write_at(p,offset,data.as_bytes()).map_err(Error::Io)?;
   publish(b"saved",out)
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
   // Files may be larger than a single terminal transfer. Stage the
   // complete, checksummed source (up to the current COW volume limit) before
   // publishing any destination metadata. This avoids partial copies.
   let mut bytes=[0u8;crate::durablefs::MAX_FILE_BYTES];
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


 #[test]fn mkfile_empty_file_is_durable_across_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /home/blank").unwrap().0,7);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,_)=run(&mut fs,&access,user(),b"cat /home/blank").unwrap();
  assert_eq!(n,0);
 }
 #[test]fn mkfile_denies_reserved_paths_and_duplicate_mutations(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /boot/system"),
             Err(Error::Denied));
  run(&mut fs,&access,user(),b"mkfile /blank").unwrap();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"mkfile /blank"),
             Err(Error::Io(FsError::Exists)));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn mkfile_small_response_and_stale_identity_do_not_write(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let generation=fs.generation();
  let mut out=[0xcc;2];
  assert_eq!(command(&mut fs,&access,user(),1,b"mkfile /blank",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0xcc;2]);
  assert_eq!(run(&mut fs,&access,Identity{pid:3,generation:2},
                 b"mkfile /blank"),Err(Error::Denied));
  assert_eq!(fs.generation(),generation);
 }


 #[test]fn truncate_and_head_survive_a_real_block_device_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkdir /home").unwrap();
  run(&mut fs,&access,user(),b"write /home/log abcdef").unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 3").unwrap();
  assert_eq!(&b[..n],b"abc");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 99").unwrap();
  assert_eq!(&b[..n],b"abcdef");
  assert_eq!(run(&mut fs,&access,user(),b"truncate /home/log").unwrap().0,9);
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert_eq!(run(&mut fs,&access,user(),b"cat /home/log").unwrap().0,0);
  run(&mut fs,&access,user(),b"append /home/log xyz").unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"head /home/log 2").unwrap();
  assert_eq!(&b[..n],b"xy");
 }
 #[test]fn truncate_denies_protected_and_missing_paths_without_mutation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let generation=fs.generation();
  assert_eq!(run(&mut fs,&access,user(),b"truncate /system/settings"),
             Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"truncate /absent"),
             Err(Error::Io(FsError::NotFound)));
  assert_eq!(fs.generation(),generation);
 }
 #[test]fn bounded_head_rejects_malformed_counts_and_stale_process_generation(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"mkfile /empty").unwrap();
  for cmd in [b"head /empty 0".as_slice(),b"head /empty x",
              b"head /empty 9999999",b"head /empty -1"]{
   assert_eq!(run(&mut fs,&access,user(),cmd),Err(Error::Invalid));
  }
  assert_eq!(run(&mut fs,&access,
    Identity{generation:2,..user()},b"head /empty 1"),Err(Error::Denied));
 }
 #[test]fn insufficient_terminal_output_never_truncates_file(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  run(&mut fs,&access,user(),b"write /note original").unwrap();
  let generation=fs.generation();
  let mut out=[0xaau8;4];
  assert_eq!(command(&mut fs,&access,user(),1,b"truncate /note",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(fs.generation(),generation);
  assert_eq!(out,[0xaa;4]);
  let (n,b)=run(&mut fs,&access,user(),b"cat /note").unwrap();
  assert_eq!(&b[..n],b"original");
 }


 #[test]fn recursive_directory_move_and_empty_remove_survive_remount(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /old").unwrap();
  run(&mut fs,&a,user(),b"mkdir /old/sub").unwrap();
  run(&mut fs,&a,user(),b"write /old/sub/note forever").unwrap();
  run(&mut fs,&a,user(),b"mvdir /old /new").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,data)=run(&mut fs,&a,user(),b"cat /new/sub/note").unwrap();
  assert_eq!(&data[..n],b"forever");
  assert_eq!(run(&mut fs,&a,user(),b"rmdir /new"),
             Err(Error::Io(FsError::Full)));
  run(&mut fs,&a,user(),b"rm /new/sub/note").unwrap();
  run(&mut fs,&a,user(),b"rmdir /new/sub").unwrap();
  run(&mut fs,&a,user(),b"rmdir /new").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  assert!(matches!(run(&mut fs,&a,user(),b"ls /new"),Err(Error::Io(_))));
 }
 #[test]fn directory_cycle_and_protected_destination_never_mutate_disk(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /safe").unwrap();
  run(&mut fs,&a,user(),b"mkdir /safe/sub").unwrap();
  let old=fs.generation();
  assert_eq!(run(&mut fs,&a,user(),b"mvdir /safe /safe/sub/cycle"),
             Err(Error::Io(FsError::InvalidName)));
  assert_eq!(run(&mut fs,&a,user(),b"mvdir /safe /system"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),old);
 }
 #[test]fn directory_operation_requires_live_grant_and_output_preflight(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"mkdir /empty").unwrap();
  let old=fs.generation();
  let mut out=[0x5bu8;3];
  assert_eq!(command(&mut fs,&a,user(),1,b"rmdir /empty",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0x5b;3]);
  assert_eq!(run(&mut fs,&a,Identity{generation:2,..user()},b"rmdir /empty"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),old);
  run(&mut fs,&a,user(),b"rmdir /empty").unwrap();
 }


 #[test]fn persistent_random_access_survives_mount_and_keeps_other_bytes(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note abcdefgh").unwrap();
  let (n,out)=run(&mut fs,&a,user(),b"pread /note 2 3").unwrap();
  assert_eq!(&out[..n],b"cde");
  run(&mut fs,&a,user(),b"pwrite /note@3 XYZ").unwrap();
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let (n,out)=run(&mut fs,&a,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"abcXYZgh");
 }
 #[test]fn random_io_rejects_stale_caller_protected_write_and_sparse_holes(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note original").unwrap();
  let generation=fs.generation();
  let stale=Identity{generation:2,..user()};
  assert_eq!(run(&mut fs,&a,stale,b"pread /note 1 2"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&a,stale,b"pwrite /note@0 bad"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&a,user(),b"pwrite /system/key@0 bad"),Err(Error::Denied));
  assert!(matches!(run(&mut fs,&a,user(),b"pwrite /note@1000 bad"),Err(Error::Io(_))));
  assert_eq!(fs.generation(),generation);
  let (n,out)=run(&mut fs,&a,user(),b"cat /note").unwrap();
  assert_eq!(&out[..n],b"original");
 }
 #[test]fn malformed_offsets_and_output_preflight_never_touch_disk(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();let a=grants();
  run(&mut fs,&a,user(),b"write /note hello").unwrap();
  let before=fs.generation();
  for cmd in [b"pread /note x 2".as_slice(),b"pread /note 0 0",
              b"pread /note 0 300",b"pwrite /note@-1 text",
              b"pwrite /note@invalid text"]{
   assert!(run(&mut fs,&a,user(),cmd).is_err());
  }
  let mut out=[0x99u8;2];
  assert_eq!(command(&mut fs,&a,user(),1,b"pwrite /note@0 hi",&mut out),
             Err(Error::OutputTooSmall));
  assert_eq!(out,[0x99;2]);
  assert_eq!(fs.generation(),before);
 }


 #[test]fn copies_multi_transfer_file_and_preserves_bytes_after_reboot(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  let mut contents=[0u8;600];
  for (i,c) in contents.iter_mut().enumerate(){*c=b'A'+(i%26) as u8;}
  fs.create("/source",&contents).unwrap();
  let (n,b)=run(&mut fs,&access,user(),b"cp /source /copy").unwrap();
  assert_eq!(&b[..n],b"copied");
  let mut fs=HierVfs::mount(fs.into_device()).unwrap();
  let mut out=[0u8;600];
  assert_eq!(fs.read("/copy",&mut out),Ok(600));
  assert_eq!(out,contents);
  // Copy must be a distinct object; source survives destination overwrite.
  fs.overwrite("/copy",b"independent").unwrap();
  assert_eq!(fs.read("/source",&mut out),Ok(600));
  assert_eq!(out,contents);
 }
 #[test]fn copy_preflight_and_stale_process_do_not_create_destination(){
  let mut fs=HierVfs::format_blank(blank()).unwrap();
  let access=grants();
  fs.create("/source",b"protected source").unwrap();
  let before=fs.generation();
  let mut output=[0x7bu8;4];
  assert_eq!(command(&mut fs,&access,user(),1,b"cp /source /copy",&mut output),
             Err(Error::OutputTooSmall));
  assert_eq!(output,[0x7b;4]);
  assert_eq!(run(&mut fs,&access,Identity{pid:3,generation:2},
             b"cp /source /copy"),Err(Error::Denied));
  assert_eq!(run(&mut fs,&access,user(),b"cp /source /system"),
             Err(Error::Denied));
  assert_eq!(fs.generation(),before);
  let mut out=[0u8;32];
  assert_eq!(fs.read("/copy",&mut out),Err(FsError::NotFound));
 }

}
