//! R041 interactive, bounded Files/Terminal command executor backed by real
//! in-memory file content. No shell processes or arbitrary program execution.
//! Call only with the identity authenticated by kernel syscall ingress.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
use crate::batch6_storage::{replace_verified,AtomicFileBackend};
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub const COMMAND_MAX:usize=192;
pub const OUTPUT_MAX:usize=512;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,Unsupported,Overflow,Io}
pub struct Terminal{line:[u8;COMMAND_MAX],length:usize}
impl Terminal{
 pub const fn new()->Self{Self{line:[0;COMMAND_MAX],length:0}}
 pub fn type_byte(&mut self,ch:u8)->Result<(),Error>{
  if !(ch==b' '||ch.is_ascii_graphic()){return Err(Error::Invalid);}
  if self.length==COMMAND_MAX{return Err(Error::Overflow);}
  self.line[self.length]=ch;self.length+=1;Ok(())
 }
 pub fn backspace(&mut self){
  if self.length>0{self.length-=1;self.line[self.length]=0;}
 }
 pub fn submit<const F:usize,const G:usize>(&mut self,auth:&Access<G>,
  caller:Identity,now:u64,fs:&mut RamFs<F>,out:&mut[u8])
  ->Result<usize,Error>{
  let len=self.length;
  let mut bytes=[0u8;COMMAND_MAX];
  bytes[..len].copy_from_slice(&self.line[..len]);
  self.length=0;self.line=[0;COMMAND_MAX];
  execute(auth,caller,now,fs,&bytes[..len],out)
 }
}
fn publish(value:&[u8],out:&mut[u8])->Result<usize,Error>{
 if value.len()>OUTPUT_MAX||value.len()>out.len(){return Err(Error::Overflow);}
 out[..value.len()].copy_from_slice(value);Ok(value.len())
}
pub fn execute<const F:usize,const G:usize>(
 auth:&Access<G>,caller:Identity,now:u64,fs:&mut RamFs<F>,
 command:&[u8],out:&mut[u8])->Result<usize,Error>{
 if command.is_empty()||command.len()>COMMAND_MAX||
  command.iter().any(|c|*c!=b' '&&!c.is_ascii_graphic()){
  return Err(Error::Invalid);
 }
 let line=core::str::from_utf8(command).map_err(|_|Error::Invalid)?.trim();
 if line.is_empty(){return Err(Error::Invalid);}
 if line=="help"{return publish(b"help pwd ls cat /path write /path text",out);}
 if line=="pwd"{return publish(b"/",out);}
 // Require actual per-process read grants rather than revealing file names.
 if line=="ls"{
  auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
  let mut tmp=[0u8;OUTPUT_MAX];let mut used=0usize;
  let mut name=[0u8;PATH_MAX];
  for idx in 0..fs.count(){
   let Some((len,_,_))=fs.entry(idx,&mut name).map_err(|_|Error::Io)?
      else {break};
   let needed=len.checked_add(1).and_then(|a|used.checked_add(a))
     .ok_or(Error::Overflow)?;
   if needed>tmp.len(){return Err(Error::Overflow);}
   tmp[used..used+len].copy_from_slice(&name[..len]);
   used+=len;tmp[used]=b'\n';used+=1;
  }
  return publish(&tmp[..used],out);
 }
 if let Some(path)=line.strip_prefix("cat "){
  auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
  let mut canonical=[0u8;PATH_MAX];
  let n=crate::vfs_path::normalize_absolute(path,&mut canonical)
   .map_err(|_|Error::Invalid)?;
  if n<=1{return Err(Error::Invalid);}
  // Read the complete snapshot before writing into caller's destination.
  let mut tmp=[0u8;OUTPUT_MAX];
  let size=fs.read_all(&canonical[..n],&mut tmp).map_err(|_|Error::Io)?;
  return publish(&tmp[..size],out);
 }
 if let Some(rest)=line.strip_prefix("write "){
  let (path,data)=rest.split_once(' ').ok_or(Error::Invalid)?;
  if path.is_empty()||data.is_empty()||data.len()>OUTPUT_MAX{
   return Err(Error::Invalid);
  }
  // Existing access check + canonicalization before actual replacement.
  replace_verified(auth,caller,now,path,data.as_bytes(),fs)
   .map_err(|_|Error::Denied)?;
  return publish(b"saved",out);
 }

 // R005: bounded file inspection and editing commands, never a shell exec.
 if let Some(path)=line.strip_prefix("head ").or_else(||line.strip_prefix("tail ")){
  auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
  let mut canonical=[0u8;PATH_MAX];
  let n=crate::vfs_path::normalize_absolute(path,&mut canonical)
   .map_err(|_|Error::Invalid)?;
  if n<=1{return Err(Error::Invalid);}
  let mut source=[0u8;crate::batch7_ramfs::CONTENT_MAX];
  let size=fs.read_all(&canonical[..n],&mut source).map_err(|_|Error::Io)?;
  let shown=size.min(OUTPUT_MAX);
  let from=if line.starts_with("tail "){size-shown}else{0};
  return publish(&source[from..from+shown],out);
 }
 if let Some(rest)=line.strip_prefix("append "){
  let (path,fragment)=rest.split_once(' ').ok_or(Error::Invalid)?;
  if path.is_empty()||fragment.is_empty()||out.len()<5{
   return Err(Error::Invalid);
  }
  auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
  let mut canonical=[0u8;PATH_MAX];
  let n=crate::vfs_path::normalize_absolute(path,&mut canonical)
   .map_err(|_|Error::Invalid)?;
  if n<=1{return Err(Error::Invalid);}
  let mut data=[0u8;crate::batch7_ramfs::CONTENT_MAX];
  let original=fs.read_all(&canonical[..n],&mut data).map_err(|_|Error::Io)?;
  let final_len=original.checked_add(fragment.len()).ok_or(Error::Overflow)?;
  if final_len>data.len(){return Err(Error::Overflow);}
  data[original..final_len].copy_from_slice(fragment.as_bytes());
  replace_verified(auth,caller,now,path,&data[..final_len],fs)
   .map_err(|_|Error::Denied)?;
  return publish(b"saved",out);
 }

 // R006: privileged Files/Terminal operations against real in-memory data.
 // Every operation is checked against a scheduler-authenticated PID/epoch.
 if let Some(rest)=line.strip_prefix("cp ").or_else(||line.strip_prefix("mv ")){
  let mut words=rest.split_ascii_whitespace();
  let source=words.next().ok_or(Error::Invalid)?;
  let dest=words.next().ok_or(Error::Invalid)?;
  if words.next().is_some(){return Err(Error::Invalid);}
  if out.len()<5{return Err(Error::Overflow);}
  auth.require(caller,FILE_READ|FILE_WRITE,now).map_err(|_|Error::Denied)?;
  let mut src=[0u8;PATH_MAX];let mut dst=[0u8;PATH_MAX];
  let a=crate::vfs_path::normalize_absolute(source,&mut src)
   .map_err(|_|Error::Invalid)?;
  let b=crate::vfs_path::normalize_absolute(dest,&mut dst)
   .map_err(|_|Error::Invalid)?;
  if a<2||b<2{return Err(Error::Invalid);}
  if line.starts_with("mv "){
   let (version,_)=fs.metadata(&src[..a]).map_err(|_|Error::Io)?;
   fs.rename_if_version(&src[..a],&dst[..b],version).map_err(|_|Error::Io)?;
  }else{
   fs.copy_new(&src[..a],&dst[..b]).map_err(|_|Error::Io)?;
  }
  return publish(b"saved",out);
 }
 if let Some(path)=line.strip_prefix("rm "){
  if out.len()<7{return Err(Error::Overflow);}
  auth.require(caller,FILE_WRITE,now).map_err(|_|Error::Denied)?;
  let mut name=[0u8;PATH_MAX];
  let n=crate::vfs_path::normalize_absolute(path,&mut name)
   .map_err(|_|Error::Invalid)?;
  if n<2{return Err(Error::Invalid);}
  let (version,_)=fs.metadata(&name[..n]).map_err(|_|Error::Io)?;
  fs.delete_if_version(&name[..n],version).map_err(|_|Error::Io)?;
  return publish(b"deleted",out);
 }
 if let Some(path)=line.strip_prefix("wc "){
  auth.require(caller,FILE_READ,now).map_err(|_|Error::Denied)?;
  let mut name=[0u8;PATH_MAX];
  let n=crate::vfs_path::normalize_absolute(path,&mut name)
   .map_err(|_|Error::Invalid)?;
  if n<2{return Err(Error::Invalid);}
  let (_,size)=fs.metadata(&name[..n]).map_err(|_|Error::Io)?;
  let mut decimal=[0u8;20];let mut value=size;let mut used=0;
  loop{
   decimal[19-used]=b'0'+(value%10) as u8;used+=1;value/=10;
   if value==0{break;}
  }
  return publish(&decimal[20-used..],out);
 }
 Err(Error::Unsupported)
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch6_security::{FILE_READ,FILE_WRITE};
 fn user()->Identity{Identity{pid:5,generation:2}}
 fn grants()->Access<1>{
  let mut a=Access::new();a.grant(user(),FILE_READ|FILE_WRITE,100).unwrap();a
 }
 #[test]fn terminal_edits_writes_reads_and_lists_actual_bytes(){
  let mut t=Terminal::new();let mut fs=RamFs::<2>::new();
  let mut output=[0u8;OUTPUT_MAX];
  for &c in b"write /memo helloo"{t.type_byte(c).unwrap();}
  t.backspace();
  assert_eq!(t.submit(&grants(),user(),1,&mut fs,&mut output),Ok(5));
  assert_eq!(&output[..5],b"saved");
  assert_eq!(execute(&grants(),user(),1,&mut fs,b"cat /memo",&mut output),Ok(5));
  assert_eq!(&output[..5],b"hello");
  assert_eq!(execute(&grants(),user(),1,&mut fs,b"ls",&mut output),Ok(6));
  assert_eq!(&output[..6],b"/memo\n");
 }
 #[test]fn wrong_generation_and_expiration_deny_reads_and_writes(){
  let mut fs=RamFs::<1>::new();let mut out=[0x55u8;OUTPUT_MAX];
  let old=Identity{generation:1,..user()};
  assert_eq!(execute(&grants(),old,1,&mut fs,b"write /memo hi",&mut out),
   Err(Error::Denied));
  assert_eq!(execute(&grants(),user(),100,&mut fs,b"ls",&mut out),
   Err(Error::Denied));
  assert_eq!(fs.count(),0);assert_eq!(out,[0x55;OUTPUT_MAX]);
 }
 #[test]fn malformed_input_is_rejected_without_output_mutation(){
  let mut fs=RamFs::<1>::new();let mut out=[0xaau8;OUTPUT_MAX];
  assert_eq!(execute(&grants(),user(),1,&mut fs,b"write /../bad hi",&mut out),
    Err(Error::Denied));
  assert_eq!(execute(&grants(),user(),1,&mut fs,b"cat ../x",&mut out),
    Err(Error::Invalid));
  assert_eq!(out,[0xaa;OUTPUT_MAX]);
 }
}

#[cfg(test)]
mod orbit_r005_terminal_tests{
 use super::*;
 fn identity()->Identity{Identity{pid:42,generation:8}}
 fn rights()->Access<1>{
  let mut a=Access::new();a.grant(identity(),FILE_READ|FILE_WRITE,100).unwrap();a
 }
 #[test]fn append_head_and_tail_operate_on_real_ram_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/note",b"first").unwrap();
  let mut output=[0u8;OUTPUT_MAX];
  assert_eq!(execute(&rights(),identity(),1,&mut fs,b"append /note second",&mut output),Ok(5));
  assert_eq!(execute(&rights(),identity(),1,&mut fs,b"head /note",&mut output),Ok(11));
  assert_eq!(&output[..11],b"firstsecond");
  assert_eq!(execute(&rights(),identity(),1,&mut fs,b"tail /note",&mut output),Ok(11));
  assert_eq!(&output[..11],b"firstsecond");
 }
 #[test]fn denied_or_malformed_edits_do_not_modify_data(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/note",b"safe").unwrap();
  let mut out=[0xaau8;OUTPUT_MAX];
  assert_eq!(execute(&rights(),Identity{generation:9,..identity()},1,
    &mut fs,b"append /note fail",&mut out),Err(Error::Denied));
  assert_eq!(execute(&rights(),identity(),1,
    &mut fs,b"append /../escape fail",&mut out),Err(Error::Invalid));
  assert_eq!(out,[0xaa;OUTPUT_MAX]);
  let mut data=[0u8;4];fs.read(b"/note",0,&mut data).unwrap();
  assert_eq!(&data,b"safe");
 }
 #[test]fn missing_file_and_expired_grants_block_inspection(){
  let mut fs=RamFs::<1>::new();let mut out=[0u8;OUTPUT_MAX];
  assert_eq!(execute(&rights(),identity(),1,&mut fs,b"head /missing",&mut out),
    Err(Error::Io));
  fs.replace(b"/note",b"hello").unwrap();
  assert_eq!(execute(&rights(),identity(),100,&mut fs,b"tail /note",&mut out),
    Err(Error::Denied));
 }
}


#[cfg(test)]
mod r006_shell_files_tests{
 use super::*;
 fn owner()->Identity{Identity{pid:8,generation:4}}
 fn acl()->Access<2>{
  let mut a=Access::new();a.grant(owner(),FILE_READ|FILE_WRITE,100).unwrap();a
 }
 #[test]fn cp_mv_wc_rm_are_real_file_mutations(){
  let mut fs=RamFs::<3>::new();let mut out=[0u8;OUTPUT_MAX];
  execute(&acl(),owner(),2,&mut fs,b"write /source hello",&mut out).unwrap();
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"cp /source /copy",&mut out),Ok(5));
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"mv /copy /renamed",&mut out),Ok(5));
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"wc /renamed",&mut out),Ok(1));
  assert_eq!(&out[..1],b"5");
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"rm /renamed",&mut out),Ok(7));
  assert_eq!(fs.count(),1);
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"cat /renamed",&mut out),Err(Error::Io));
  assert_eq!(execute(&acl(),owner(),2,&mut fs,b"cat /source",&mut out),Ok(5));
 }
 #[test]fn bad_paths_capacity_and_duplicate_target_do_not_drop_source(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"safe").unwrap();
  fs.replace(b"/b",b"keep").unwrap();
  let mut output=[0xaau8;OUTPUT_MAX];
  assert_eq!(execute(&acl(),owner(),1,&mut fs,b"mv /a /b",&mut output),Err(Error::Io));
  assert_eq!(execute(&acl(),owner(),1,&mut fs,b"cp /a /c",&mut output),Err(Error::Io));
  assert_eq!(execute(&acl(),owner(),1,&mut fs,b"rm /../a",&mut output),Err(Error::Invalid));
  let mut verify=[0u8;4];fs.read(b"/a",0,&mut verify).unwrap();
  assert_eq!(&verify,b"safe");
  assert_eq!(output,[0xaa;OUTPUT_MAX]);
 }
 #[test]fn mismatched_generation_and_expired_rights_block_all_mutations(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"safe").unwrap();
  let mut output=[0x55u8;OUTPUT_MAX];
  let stale=Identity{generation:5,..owner()};
  for cmd in [b"cp /a /b".as_slice(),b"mv /a /b",b"rm /a",b"wc /a"]{
   assert_eq!(execute(&acl(),stale,1,&mut fs,cmd,&mut output),Err(Error::Denied));
   assert_eq!(execute(&acl(),owner(),100,&mut fs,cmd,&mut output),Err(Error::Denied));
  }
  assert_eq!(fs.count(),1);
 }
}
