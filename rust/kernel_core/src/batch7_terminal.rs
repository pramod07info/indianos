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
