//! R029/14: group textual compiler output by file and severity with a release gate.
//! Strict bounded parser for real diagnostic lines; unparseable lines are not ignored.
use super::Error;
use crate::batch7_ramfs::PATH_MAX;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Severity{Error,Warning}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Diagnostic{pub path:[u8;PATH_MAX],pub len:usize,pub line:usize,pub severity:Severity}
pub const EMPTY:Diagnostic=Diagnostic{path:[0;PATH_MAX],len:0,line:0,severity:Severity::Error};
pub fn parse<const N:usize>(log:&[u8],out:&mut[Diagnostic;N])
 ->Result<usize,Error>{
 let mut stage=[EMPTY;N];let mut count=0;
 for row in log.split(|&b|b==b'\n'){
  if row.is_empty(){continue;}
  let mut fields=row.splitn(3,|&b|b==b':');
  let path=fields.next().ok_or(Error::Invalid)?;
  let line=fields.next().ok_or(Error::Invalid)?;
  let message=fields.next().ok_or(Error::Invalid)?;
  if path.len()<2||path.len()>PATH_MAX||path[0]!=b'/'||
   line.is_empty()||!line.iter().all(u8::is_ascii_digit){return Err(Error::Invalid);}
  let line=core::str::from_utf8(line).map_err(|_|Error::Invalid)?
   .parse::<usize>().map_err(|_|Error::Invalid)?;
  let severity=if message.starts_with(b"error:"){Severity::Error}
   else if message.starts_with(b"warning:"){Severity::Warning}
   else{return Err(Error::Invalid)};
  if count==N{return Err(Error::Full);}
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  stage[count]=Diagnostic{path:p,len:path.len(),line,severity};count+=1;
 }
 *out=stage;Ok(count)
}
pub fn release(items:&[Diagnostic])->Result<(),Error>{
 if items.iter().any(|d|d.severity==Severity::Error){return Err(Error::NotReady);}
 Ok(())
}
pub fn errors(items:&[Diagnostic])->usize{
 items.iter().filter(|d|d.severity==Severity::Error).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compiler_log_groups_failure_and_warning_by_source(){
  let mut out=[EMPTY;2];
  let n=parse(b"/p/a.rs:7:error:bad\n/p/a.rs:8:warning:unused\n",&mut out).unwrap();
  assert_eq!(n,2);assert_eq!(errors(&out[..n]),1);
  assert_eq!(release(&out[..n]),Err(Error::NotReady));
 }
 #[test]fn malformed_diagnostic_does_not_appear_as_green_build(){
  let mut out=[EMPTY;1];
  assert_eq!(parse(b"success!",&mut out),Err(Error::Invalid));
 }
}