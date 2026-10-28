//! R021/08 source-lint rules for real RamFs project files.
//! Lint findings are bounded and non-destructive, not a full language compiler.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Rule{TrailingSpace,TabIndent,LineTooLong,TodoMarker}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Finding{pub line:u32,pub column:u32,pub rule:Rule}
pub const EMPTY:Finding=Finding{line:0,column:0,rule:Rule::TodoMarker};
pub fn lint<const N:usize>(data:&[u8],max_line:usize,
 out:&mut[Finding;N])->Result<usize,Error>{
 if max_line==0||max_line>CONTENT_MAX{return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut count=0;
 let mut start=0;let mut line=1u32;
 while start<data.len(){
  let end=data[start..].iter().position(|b|*b==b'\n')
   .map(|x|start+x).unwrap_or(data.len());
  let text=&data[start..end];
  let mut add=|col:usize,rule:Rule|->Result<(),Error>{
   if count==N{return Err(Error::Full);}
   stage[count]=Finding{line,column:col as u32,rule};count+=1;Ok(())
  };
  if text.len()>max_line{add(max_line+1,Rule::LineTooLong)?;}
  if text.first()==Some(&b'\t'){add(1,Rule::TabIndent)?;}
  if text.last()==Some(&b' '){add(text.len(),Rule::TrailingSpace)?;}
  if let Some(pos)=text.windows(4).position(|w|w==b"TODO"){
   add(pos+1,Rule::TodoMarker)?;
  }
  if end==data.len(){break;}
  start=end+1;line=line.checked_add(1).ok_or(Error::Overflow)?;
 }*out=stage;Ok(count)
}
pub fn lint_file<const F:usize,const N:usize>(fs:&RamFs<F>,
 path:&[u8],max_line:usize,out:&mut[Finding;N])->Result<usize,Error>{
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut bytes=[0u8;CONTENT_MAX];
 let n=fs.read(path,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
 lint(&bytes[..n],max_line,out)
}
pub fn count_rule(items:&[Finding],rule:Rule)->usize{
 items.iter().filter(|f|f.rule==rule).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn finds_todos_tabs_trailing_spaces_and_long_lines(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/main.rs",b"\tlet a = 1; \n// TODO fix").unwrap();
  let mut out=[EMPTY;6];
  let n=lint_file(&fs,b"/src/main.rs",9,&mut out).unwrap();
  assert_eq!(n,5);
  assert_eq!(count_rule(&out[..n],Rule::TodoMarker),1);
 }
 #[test]fn too_many_findings_leave_inspection_results_unchanged(){
  let mut out=[Finding{line:99,..EMPTY};1];
  assert_eq!(lint(b"\tTODO \n",2,&mut out),Err(Error::Full));
  assert_eq!(out[0].line,99);
 }
 #[test]fn missing_file_and_unbounded_limit_are_rejected(){
  let fs=RamFs::<1>::new();let mut out=[EMPTY;4];
  assert_eq!(lint_file(&fs,b"/missing",40,&mut out),Err(Error::Missing));
  assert_eq!(lint(b"hello",0,&mut out),Err(Error::Invalid));
 }
}
