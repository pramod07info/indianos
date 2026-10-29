//! R031/11: run a bounded sequence of authenticated read-only terminal commands.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::batch5_session::Identity;
use crate::workstation_r025::agent_02::Ticket;
use crate::workstation_r029::agent_10;
/// Results are published only after the complete command batch succeeds.
pub fn batch<const F:usize>(fs:&RamFs<F>,who:Identity,ticket:Ticket,
 now:u64,commands:&[&[u8]],out:&mut[u8])->Result<usize,Error>{
 if commands.is_empty()||commands.len()>8{return Err(Error::Invalid);}
 let mut buffer=[0u8;CONTENT_MAX];let mut n=0;
 for (i,command) in commands.iter().enumerate(){
  if i>0{if n==CONTENT_MAX{return Err(Error::Full);}buffer[n]=b'\n';n+=1;}
  let mut result=[0u8;CONTENT_MAX];
  let bytes=agent_10::run(fs,who,ticket,now,command,&mut result)
   .map_err(|_|Error::Denied)?;
  if n+bytes>CONTENT_MAX{return Err(Error::Full);}
  buffer[n..n+bytes].copy_from_slice(&result[..bytes]);n+=bytes;
 }
 if n>out.len(){return Err(Error::Full);}
 out[..n].copy_from_slice(&buffer[..n]);Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r025::agent_02 as rights;
 #[test]fn batch_runs_real_guest_read_commands_with_one_ticket(){
  let who=Identity{pid:4,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"hello").unwrap();
  let ticket=rights::grant(true,who,&fs,b"/p",rights::READ,30).unwrap();
  let mut out=[0;32];let n=batch(&fs,who,ticket,1,&[&b"cat /p/a"[..],&b"wc /p/a"[..]],&mut out).unwrap();
  assert_eq!(&out[..n],b"hello\n1");
 }
 #[test]fn denied_second_command_does_not_expose_partial_results(){
  let who=Identity{pid:4,generation:1};
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"hello").unwrap();
  let ticket=rights::grant(true,who,&fs,b"/p",rights::READ,30).unwrap();
  let mut out=[0xAA;32];
  assert_eq!(batch(&fs,who,ticket,1,&[&b"cat /p/a"[..],&b"cat /q/private"[..]],&mut out),
   Err(Error::Denied));assert_eq!(out,[0xAA;32]);
 }
}