//! R029/10: typed terminal dispatcher executes allowlisted operations on real guest files.
//! Untrusted shell expressions are rejected by R028 parser; requires scoped read grant.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::workstation_r028::agent_10 as parser;
use crate::workstation_r025::{agent_10 as builtin,agent_02 as permissions};
pub fn run<const F:usize>(fs:&RamFs<F>,who:Identity,
 ticket:permissions::Ticket,now:u64,input:&[u8],
 out:&mut[u8])->Result<usize,Error>{
 let command=parser::parse(input).map_err(|_|Error::Invalid)?;
 let path=&command.path[..command.len];let arg=&command.argument[..command.arg_len];
 permissions::authorize(ticket,who,now,permissions::READ,path)
  .map_err(|_|Error::Denied)?;
 let action=match command.command{
  parser::Command::Cat=>builtin::Command::Cat,
  parser::Command::Stat=>builtin::Command::Stat,
  parser::Command::Wc=>builtin::Command::WordCount,
  parser::Command::Head=>builtin::Command::Head,
  parser::Command::Grep=>builtin::Command::Find
 };
 let mut buffer=[0u8;CONTENT_MAX];
 let n=builtin::run(fs,path,action,arg,&mut buffer).map_err(|_|Error::Invalid)?;
 if n>out.len(){return Err(Error::Full);}
 out[..n].copy_from_slice(&buffer[..n]);Ok(n)
}
pub fn format_command(input:&[u8])->Result<parser::Parsed,Error>{
 parser::parse(input).map_err(|_|Error::Invalid)
}
#[cfg(test)]mod tests{
 use super::*;
 fn owner()->Identity{Identity{pid:9,generation:1}}
 #[test]fn authorized_terminal_executes_real_file_grep_and_wc(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/log",b"ok\nerror\n").unwrap();
  let t=permissions::grant(true,owner(),&fs,b"/p",permissions::READ,20).unwrap();
  let mut out=[0;32];let n=run(&fs,owner(),t,1,b"grep /p/log error",&mut out).unwrap();
  assert_eq!(&out[..n],b"error\n");
  let n=run(&fs,owner(),t,1,b"wc /p/log",&mut out).unwrap();
  assert_eq!(&out[..n],b"2");
 }
 #[test]fn shell_injection_and_cross_project_read_denied(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"safe").unwrap();
  let t=permissions::grant(true,owner(),&fs,b"/p",permissions::READ,20).unwrap();
  assert_eq!(run(&fs,owner(),t,1,b"cat /p/a;rm /p/a",&mut[0;9]),Err(Error::Invalid));
  assert_eq!(run(&fs,owner(),t,1,b"cat /q/a",&mut[0;9]),Err(Error::Denied));
 }
}