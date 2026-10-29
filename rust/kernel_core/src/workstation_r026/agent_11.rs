//! R026/11: redirect typed terminal output to real files, with optimistic revision guard.
//! No external shell execution or arbitrary commands.
use super::{Error,agent_10};
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
use crate::workstation_r025::agent_02 as access;
use crate::workstation_r025::agent_10 as builtins;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Output{Replace,Append,CreateOnly}
pub fn redirect<const F:usize>(fs:&mut RamFs<F>,owner:Identity,
 ticket:access::Ticket,now:u64,revision:u64,
 source:&[u8],command:builtins::Command,param:&[u8],
 destination:&[u8],mode:Output)->Result<u64,Error>{
 if fs.version()!=revision{return Err(Error::Stale);}
 if destination.len()<2||destination.len()>PATH_MAX||source==destination{
  return Err(Error::Invalid);
 }
 access::authorize(ticket,owner,now,access::WRITE,destination)
  .map_err(|_|Error::Denied)?;
 let mut stdout=[0u8;CONTENT_MAX];
 let count=builtins::run(fs,source,command,param,&mut stdout)?;
 let existing=fs.metadata(destination).ok();
 if mode==Output::CreateOnly&&existing.is_some(){return Err(Error::Conflict);}
 let old=if mode==Output::Append{
  existing.ok_or(Error::Missing)?.1
 }else{0};
 let size=old.checked_add(count).ok_or(Error::Overflow)?;
 if size>CONTENT_MAX{return Err(Error::Full);}
 let mut data=[0u8;CONTENT_MAX];
 if old>0{fs.read(destination,0,&mut data[..old])
  .map_err(|_|Error::Missing)?;}
 data[old..size].copy_from_slice(&stdout[..count]);
 fs.replace(destination,&data[..size]).map_err(|_|Error::Full)
}
pub fn output_size<const F:usize>(fs:&RamFs<F>,source:&[u8],
 command:builtins::Command,param:&[u8])->Result<usize,Error>{
 let mut out=[0u8;CONTENT_MAX];
 builtins::run(fs,source,command,param,&mut out)
}
#[cfg(test)]mod tests{
 use super::*;fn owner()->Identity{Identity{pid:7,generation:1}}
 #[test]fn redirected_terminal_file_output_is_real_ramfs_file(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/log",b"one\ntwo\n").unwrap();
  let ticket=access::grant(true,owner(),&fs,b"/p",access::WRITE,50).unwrap();
  redirect(&mut fs,owner(),ticket,1,1,b"/p/log",
   builtins::Command::WordCount,b"",b"/p/count",Output::CreateOnly).unwrap();
  let mut out=[0;1];fs.read(b"/p/count",0,&mut out).unwrap();
  assert_eq!(&out,b"2");
 }
 #[test]fn redirect_cannot_overwrite_existing_file_with_create_only(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"source").unwrap();
  fs.replace(b"/p/b",b"keep").unwrap();
  let t=access::grant(true,owner(),&fs,b"/p",access::WRITE,50).unwrap();
  assert_eq!(redirect(&mut fs,owner(),t,1,2,b"/p/a",
   builtins::Command::Stat,b"",b"/p/b",Output::CreateOnly),Err(Error::Conflict));
 }
 #[test]fn missing_write_scope_denies_output_change(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"source").unwrap();
  let t=access::grant(true,owner(),&fs,b"/p",access::READ,50).unwrap();
  assert_eq!(redirect(&mut fs,owner(),t,1,1,b"/p/a",
   builtins::Command::Stat,b"",b"/p/b",Output::CreateOnly),Err(Error::Denied));
 }
}
