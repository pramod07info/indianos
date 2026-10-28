//! R024/14: bounded read-only terminal pipelines over actual guest file bytes.
//! Only safe typed operations (cat/grep/lines) are available; no external shell execution.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Filter{Contains,StartsWith,Nonempty}
pub fn filter<const B:usize>(text:&[u8],pattern:&[u8],
 mode:Filter,out:&mut[u8;B])->Result<(usize,usize),Error>{
 if pattern.is_empty()&&mode!=Filter::Nonempty{return Err(Error::Invalid);}
 let mut stage=[0u8;B];let mut written=0;let mut matches=0;
 for part in text.split_inclusive(|b|*b==b'\n'){
  let line=part.strip_suffix(b"\n").unwrap_or(part);
  let include=match mode{
   Filter::Contains=>line.windows(pattern.len()).any(|w|w==pattern),
   Filter::StartsWith=>line.starts_with(pattern),
   Filter::Nonempty=>!line.is_empty()
  };
  if include{
   if written+part.len()>B{return Err(Error::Full);}
   stage[written..written+part.len()].copy_from_slice(part);
   written+=part.len();matches+=1;
  }
 }
 *out=stage;Ok((written,matches))
}
pub fn cat_grep<const F:usize,const B:usize>(fs:&RamFs<F>,
 path:&[u8],needle:&[u8],out:&mut[u8;B])
 ->Result<(usize,usize),Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 filter(&source[..size],needle,Filter::Contains,out)
}
pub fn head<const F:usize,const B:usize>(fs:&RamFs<F>,
 path:&[u8],lines:usize,out:&mut[u8;B])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 let mut n=0usize;let mut count=0;
 for part in source[..size].split_inclusive(|b|*b==b'\n'){
  if count>=lines{break;}
  if n+part.len()>B{return Err(Error::Full);}
  n+=part.len();count+=1;
 }
 out[..n].copy_from_slice(&source[..n]);Ok(n)
}
pub fn line_count(data:&[u8])->usize{
 if data.is_empty(){return 0;}
 data.iter().filter(|&&b|b==b'\n').count()+
  usize::from(data.last()!=Some(&b'\n'))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn grep_and_head_work_with_real_project_logs(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/build.log",b"ok first\nerror one\nok second\n").unwrap();
  let mut out=[0u8;64];
  let (n,k)=cat_grep(&fs,b"/build.log",b"error",&mut out).unwrap();
  assert_eq!((n,k),(10,1));assert_eq!(&out[..n],b"error one\n");
  let n=head(&fs,b"/build.log",1,&mut out).unwrap();
  assert_eq!(&out[..n],b"ok first\n");
 }
 #[test]fn short_pipeline_buffer_never_exposes_partial_old_source(){
  let mut out=[88;3];
  assert_eq!(filter(b"hello\n",b"hello",Filter::Contains,&mut out),
   Err(Error::Full));assert_eq!(out,[88;3]);
 }
 #[test]fn empty_search_and missing_files_are rejected(){
  let fs=RamFs::<1>::new();let mut out=[0;64];
  assert_eq!(cat_grep(&fs,b"/missing",b"error",&mut out),Err(Error::Missing));
  assert_eq!(filter(b"foo",b"",Filter::Contains,&mut out),Err(Error::Invalid));
 }
}
