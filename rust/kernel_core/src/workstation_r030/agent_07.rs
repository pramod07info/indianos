//! R030/07: line-preserving real source preview and bounded scrolling.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn lines<const F:usize>(fs:&RamFs<F>,path:&[u8],first:usize,
 count:usize,out:&mut[u8])->Result<(usize,usize),Error>{
 if first==0||count==0{return Err(Error::Invalid);}
 let (_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..n])
  .map_err(|_|Error::Missing)?;
 let mut at=0;let mut line=1;let mut selected=0;let mut write=0;
 while at<n{
  let start=at;while at<n&&src[at]!=b'\n'{at+=1;}
  if at<n{at+=1;}
  if line>=first&&selected<count{
   let len=at-start;if write+len>out.len(){return Err(Error::Full);}
   out[write..write+len].copy_from_slice(&src[start..at]);
   write+=len;selected+=1;
  }
  line+=1;
  if selected==count{break;}
 }
 if selected==0{return Err(Error::Missing);}Ok((write,selected))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn viewport_reads_real_file_lines_with_exact_newlines(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"one\ntwo\nthree").unwrap();
  let mut out=[0;16];let (n,lines)=lines(&fs,b"/p/a",2,2,&mut out).unwrap();
  assert_eq!(lines,2);assert_eq!(&out[..n],b"two\nthree");
 }
 #[test]fn truncated_viewport_reports_full_without_partial_overwrite_claim(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"large\n").unwrap();
  assert_eq!(lines(&fs,b"/p/a",1,1,&mut[0;2]),Err(Error::Full));
  assert_eq!(lines(&fs,b"/p/a",3,1,&mut[0;8]),Err(Error::Missing));
 }
}