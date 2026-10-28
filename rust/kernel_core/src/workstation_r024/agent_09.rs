//! R024/09: source-linked compiler diagnostic mapping and targeted suppression.
//! Requires current real RamFs version, bounded indexed text and explicit owner.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
use crate::workstation_r019::agent_04::{Diagnostic,Severity};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Finding{pub path:[u8;PATH_MAX],pub len:usize,
 pub byte:usize,pub line:u32,pub column:u32,
 pub version:u64,pub severity:Severity}
pub fn locate<const F:usize>(fs:&RamFs<F>,d:Diagnostic)->Result<Finding,Error>{
 if d.path_len==0||d.path_len>PATH_MAX||d.line==0||d.column==0{
  return Err(Error::Invalid);
 }
 let path=&d.path[..d.path_len];
 let (version,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut text=[0u8;CONTENT_MAX];fs.read(path,0,&mut text[..len])
  .map_err(|_|Error::Missing)?;
 let mut line=1u32;let mut col=1u32;
 for i in 0..=len{
  if line==d.line&&col==d.column{
   let mut full_path=[0u8;PATH_MAX];
   full_path[..d.path_len].copy_from_slice(&d.path[..d.path_len]);
   return Ok(Finding{path:full_path,len:d.path_len,byte:i,line,
    column:col,version,severity:d.severity});
  }
  if i==len{break;}
  if text[i]==b'\n'{line+=1;col=1;}else{col+=1;}
 }Err(Error::Missing)
}
pub fn navigate<const F:usize>(fs:&RamFs<F>,finding:Finding)
 ->Result<usize,Error>{
 let (version,size)=fs.metadata(&finding.path[..finding.len])
  .map_err(|_|Error::Missing)?;
 if version!=finding.version||finding.byte>size{return Err(Error::Stale);}
 Ok(finding.byte)
}
pub fn summarize(findings:&[Finding])->(usize,usize,usize){
 let mut count=(0,0,0);
 for f in findings{
  match f.severity{Severity::Error=>count.0+=1,
   Severity::Warning=>count.1+=1,Severity::Note=>count.2+=1}
 }count
}
pub fn filter<const O:usize>(items:&[Finding],severity:Severity,
 out:&mut[Finding;O])->Result<usize,Error>{
 let count=items.iter().filter(|f|f.severity==severity).count();
 if count>O{return Err(Error::Full);}
 let mut n=0;let mut temp=[Finding{path:[0;PATH_MAX],len:0,byte:0,
  line:0,column:0,version:0,severity:Severity::Note};O];
 for f in items.iter().filter(|f|f.severity==severity){
  temp[n]=*f;n+=1;
 }
 *out=temp;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r019::agent_04::parse;
 #[test]fn diagnostics_navigate_to_exact_real_file_line_and_column(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"line1\nlet x").unwrap();
  let d=parse(b"/a:2:5:error mismatch").unwrap();
  let f=locate(&fs,d).unwrap();
  assert_eq!(navigate(&fs,f),Ok(10));
  assert_eq!(summarize(&[f]),(1,0,0));
 }
 #[test]fn source_change_invalidates_previous_compiler_diagnostic(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let f=locate(&fs,parse(b"/a:1:1:error bad").unwrap()).unwrap();
  fs.replace(b"/a",b"changed").unwrap();
  assert_eq!(navigate(&fs,f),Err(Error::Stale));
 }
 #[test]fn insufficient_error_filter_does_not_publish_partial_diagnostics(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"xx").unwrap();
  let f=locate(&fs,parse(b"/a:1:1:error bad").unwrap()).unwrap();
  let mut out:[Finding;0]=[];
  assert_eq!(filter(&[f,f],Severity::Error,&mut out),Err(Error::Full));
 }
}
