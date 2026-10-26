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

pub const MAX_COMMAND:usize=192;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Io(FsError),OutputTooSmall,Unknown}
fn path<'a>(input:&str,buffer:&'a mut [u8;64])->Result<&'a str,Error>{
 let n=normalize_absolute(input,buffer).map_err(|_|Error::Invalid)?;
 if n<2{return Err(Error::Invalid);}
 core::str::from_utf8(&buffer[..n]).map_err(|_|Error::Invalid)
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
 if line=="help"{return publish(b"pwd ls /dir cat /path write /path text mkdir /dir mv /old /new",out);}
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
   fs.mkdir(p).map_err(Error::Io)?;
   publish(b"created",out)
  }
  "write"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (raw_path,data)=raw.split_once(' ').ok_or(Error::Invalid)?;
   if data.is_empty()||data.len()>MAX_TRANSFER{return Err(Error::Invalid);}
   let p=path(raw_path,&mut name)?;
   // Existing files are COW-overwritten; absent files are created. DurableFs
   // atomically publishes metadata; BlockDevice must flush each write.
   match fs.overwrite(p,data.as_bytes()){
    Ok(())=>{},
    Err(FsError::NotFound)=>fs.create(p,data.as_bytes()).map_err(Error::Io)?,
    Err(e)=>return Err(Error::Io(e)),
   };
   publish(b"saved",out)
  }
  "mv"=>{
   auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
   if out.len()<5{return Err(Error::OutputTooSmall);}
   let (left,right)=raw.split_once(' ').ok_or(Error::Invalid)?;
   let from=path(left,&mut name)?;
   let mut target=[0u8;64];
   let to=path(right,&mut target)?;
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
}
