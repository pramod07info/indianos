//! R031/08: real-file line endings / trailing whitespace cleanup, retaining CRLF.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn clean<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 revision:u64)->Result<(usize,u64),Error>{
 let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..n])
  .map_err(|_|Error::Missing)?;
 core::str::from_utf8(&src[..n]).map_err(|_|Error::Invalid)?;
 let mut dst=[0u8;CONTENT_MAX];let(mut start,mut w,mut removed)=(0usize,0usize,0usize);
 while start<n{
  let mut end=start;while end<n&&src[end]!=b'\n'{end+=1;}
  let line_end=end;let cr=line_end>start&&src[line_end-1]==b'\r';
  let body_end=if cr{line_end-1}else{line_end};
  let mut trimmed=body_end;
  while trimmed>start&&(src[trimmed-1]==b' '||src[trimmed-1]==b'\t'){
   trimmed-=1;
  }
  removed+=body_end-trimmed;
  let bytes=trimmed-start;
  dst[w..w+bytes].copy_from_slice(&src[start..trimmed]);w+=bytes;
  if cr{dst[w]=b'\r';w+=1;}
  if end<n{dst[w]=b'\n';w+=1;}
  start=if end<n{end+1}else{end};
 }
 if removed==0{return Ok((0,revision));}
 let new_rev=fs.replace(path,&dst[..w]).map_err(|_|Error::Full)?;
 Ok((removed,new_rev))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn trims_per_line_whitespace_and_preserves_windows_eol(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a  \r\nb \n").unwrap();
  assert_eq!(clean(&mut fs,b"/p/a",1),Ok((3,2)));
  let mut buf=[0;5];fs.read(b"/p/a",0,&mut buf).unwrap();
  assert_eq!(&buf,b"a\r\nb\n");
 }
 #[test]fn unchanged_file_keeps_generation_and_stale_edit_fails(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a\n").unwrap();
  assert_eq!(clean(&mut fs,b"/p/a",1),Ok((0,1)));
  assert_eq!(clean(&mut fs,b"/p/a",2),Err(Error::Stale));
 }
}