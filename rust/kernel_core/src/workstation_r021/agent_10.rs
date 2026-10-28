//! R021/10 compiler diagnostics jump-to-source with verified line/column.
//! Resolves diagnostic text against real, current RamFs bytes.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::workstation_r019::agent_04::{Diagnostic,Severity};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Target{pub byte:usize,pub line:u32,pub column:u32,
 pub severity:Severity,pub file_version:u64}
pub fn resolve<const F:usize>(fs:&RamFs<F>,
 d:&Diagnostic,expected_version:u64)->Result<Target,Error>{
 if d.path_len==0||d.line==0||d.column==0||
  d.path_len>d.path.len(){return Err(Error::Invalid);}
 let path=&d.path[..d.path_len];
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if expected_version!=version{return Err(Error::Stale);}
 let mut data=[0u8;CONTENT_MAX];
 let n=fs.read(path,0,&mut data[..size]).map_err(|_|Error::Missing)?;
 let mut line=1u32;let mut col=1u32;
 for i in 0..=n{
  if line==d.line&&col==d.column{
   return Ok(Target{byte:i,line,column:col,severity:d.severity,file_version:version});
  }
  if i==n{break;}
  if data[i]==b'\n'{line+=1;col=1;}else{col+=1;}
 }Err(Error::Missing)
}
pub fn batch<const F:usize,const N:usize>(fs:&RamFs<F>,
 diagnostics:&[Diagnostic],versions:&[u64],
 out:&mut[Target;N])->Result<usize,Error>{
 if diagnostics.len()!=versions.len()||diagnostics.len()>N{
  return Err(Error::Full);
 }
 let init=Target{byte:0,line:0,column:0,severity:Severity::Note,file_version:0};
 let mut staged=[init;N];
 for (i,d) in diagnostics.iter().enumerate(){
  staged[i]=resolve(fs,d,versions[i])?;
 }
 *out=staged;Ok(diagnostics.len())
}
pub fn first_error<'a>(targets:&'a[Target])->Option<&'a Target>{
 targets.iter().find(|t|t.severity==Severity::Error)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r019::agent_04::parse;
 #[test]fn compiler_error_jumps_to_actual_editor_byte_offset(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/main.rs",b"line1\nlet bad").unwrap();
  let d=parse(b"/main.rs:2:5:error invalid expression").unwrap();
  let t=resolve(&fs,&d,1).unwrap();assert_eq!(t.byte,10);
  assert_eq!(t.severity,Severity::Error);
 }
 #[test]fn stale_diagnostic_does_not_navigate_changed_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abc").unwrap();
  let d=parse(b"/a:1:2:error bad").unwrap();
  fs.replace(b"/a",b"changed").unwrap();
  assert_eq!(resolve(&fs,&d,1),Err(Error::Stale));
 }
 #[test]fn invalid_batch_target_is_fail_atomic(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let d=parse(b"/a:1:1:error x").unwrap();
  let empty=Target{byte:99,line:0,column:0,severity:Severity::Note,file_version:0};
  let mut out=[empty;2];
  assert_eq!(batch(&fs,&[d,d],&[1,2],&mut out),Err(Error::Stale));
  assert_eq!(out,[empty;2]);
 }
}
