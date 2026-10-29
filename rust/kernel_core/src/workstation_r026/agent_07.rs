//! R026/07: atomic multi-source literal replacement in RamFs.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
fn occurrences(text:&[u8],needle:&[u8])->usize{
 if needle.is_empty(){0}else{text.windows(needle.len()).filter(|x|*x==needle).count()}
}
pub fn replace<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 expected:u64,root:&[u8],needle:&[u8],with:&[u8])->Result<usize,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if root.len()<2||root[0]!=b'/'||needle.is_empty()||
  needle.len()>CONTENT_MAX||with.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let mut image=[0u8;B];let n=fs.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;
 let mut count=0;
 for index in 0..fs.count(){
  let mut name=[0;PATH_MAX];
  let (len,size,_)=fs.entry(index,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !name[..len].starts_with(root)||name.get(root.len())!=Some(&b'/')||
   !name[..len].ends_with(b".rs"){continue;}
  let mut src=[0u8;CONTENT_MAX];fs.read(&name[..len],0,&mut src[..size])
   .map_err(|_|Error::Missing)?;
  if occurrences(&src[..size],needle)==0{continue;}
  let mut dst=[0u8;CONTENT_MAX];let mut i=0usize;let mut n=0usize;
  while i<size{
   if i+needle.len()<=size&&&src[i..i+needle.len()]==needle{
    if n+with.len()>CONTENT_MAX{return Err(Error::Full);}
    dst[n..n+with.len()].copy_from_slice(with);n+=with.len();i+=needle.len();count+=1;
   }else{
    if n==CONTENT_MAX{return Err(Error::Full);}
    dst[n]=src[i];n+=1;i+=1;
   }
  }
  staged.replace(&name[..len],&dst[..n]).map_err(|_|Error::Full)?;
 }
 if count==0{return Ok(0);}
 let mut last=[0u8;B];let n=staged.export_snapshot(&mut last)
  .map_err(|_|Error::Full)?;
 fs.import_snapshot(&last[..n]).map_err(|_|Error::Invalid)?;Ok(count)
}
pub fn count<const F:usize>(fs:&RamFs<F>,root:&[u8],
 needle:&[u8])->Result<usize,Error>{
 if needle.is_empty(){return Err(Error::Invalid);}
 let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root){continue;}
  let mut text=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut text[..size])
   .map_err(|_|Error::Missing)?;
  n+=occurrences(&text[..size],needle);
 }Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn replaces_real_identifier_across_two_rust_files(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/a.rs",b"foo foo").unwrap();
  fs.replace(b"/p/b.rs",b"foo").unwrap();fs.replace(b"/q/c.rs",b"foo").unwrap();
  assert_eq!(replace::<3,2048>(&mut fs,3,b"/p",b"foo",b"bar"),Ok(3));
  let mut out=[0;7];fs.read(b"/p/a.rs",0,&mut out).unwrap();assert_eq!(&out,b"bar bar");
 }
 #[test]fn stale_project_revision_refuses_all_source_edits(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"a").unwrap();
  assert_eq!(replace::<1,512>(&mut fs,0,b"/p",b"a",b"b"),Err(Error::Stale));
 }
 #[test]fn replacement_over_content_limit_preserves_source_bytes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"xx").unwrap();
  assert_eq!(replace::<1,1024>(&mut fs,1,b"/p",b"x",&[3;CONTENT_MAX]),
   Err(Error::Full));
  let mut out=[0;2];fs.read(b"/p/a.rs",0,&mut out).unwrap();assert_eq!(&out,b"xx");
 }
}
