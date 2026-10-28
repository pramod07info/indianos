//! R023/03: actual multi-file search-and-replace with preview and atomic commit.
//! Literal byte search is NOT semantic renaming; no silent partial writes.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Report{pub files:usize,pub occurrences:usize,pub bytes_delta:isize}
fn replace_file<const B:usize>(data:&[u8],old:&[u8],new:&[u8],
 output:&mut[u8;B])->Result<(usize,usize),Error>{
 let mut src=0usize;let mut dst=0usize;let mut changed=0usize;
 while src<data.len(){
  let matches=src+old.len()<=data.len()&&&data[src..src+old.len()]==old;
  let take=if matches{new}else{&data[src..src+1]};
  let end=dst.checked_add(take.len()).ok_or(Error::Overflow)?;
  if end>B{return Err(Error::Full);}
  output[dst..end].copy_from_slice(take);dst=end;
  src+=if matches{old.len()}else{1};
  if matches{changed+=1;}
 }
 Ok((dst,changed))
}
fn valid(old:&[u8],new:&[u8])->Result<(),Error>{
 if old.is_empty()||old.len()>128||new.len()>128||old==new{
  Err(Error::Invalid)
 }else{Ok(())}
}
pub fn replace_all<const F:usize,const IMAGE:usize>(
 fs:&mut RamFs<F>,expected:u64,prefix:&[u8],
 old:&[u8],new:&[u8])->Result<Report,Error>{
 valid(old,new)?;
 if expected!=fs.version(){return Err(Error::Stale);}
 if prefix.is_empty()||prefix[0]!=b'/'||prefix.len()>PATH_MAX{
  return Err(Error::Invalid);
 }
 let mut image=[0u8;IMAGE];let n=fs.export_snapshot(&mut image)
  .map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;
 let mut report=Report{files:0,occurrences:0,bytes_delta:0};
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix){continue;}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(&path[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  let mut result=[0u8;CONTENT_MAX];
  let (next,occurrences)=replace_file(&bytes[..size],old,new,&mut result)?;
  if occurrences>0{
   staged.replace(&path[..len],&result[..next]).map_err(|_|Error::Full)?;
   report.files+=1;report.occurrences+=occurrences;
   report.bytes_delta+=(next as isize)-(size as isize);
  }
 }
 if report.files==0{return Ok(report);}
 let mut final_image=[0u8;IMAGE];let n=staged.export_snapshot(&mut final_image)
  .map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_image[..n]).map_err(|_|Error::Invalid)?;
 Ok(report)
}
pub fn preview<const F:usize>(fs:&RamFs<F>,prefix:&[u8],old:&[u8],
 new:&[u8])->Result<Report,Error>{
 valid(old,new)?;
 let mut result=Report{files:0,occurrences:0,bytes_delta:0};
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix){continue;}
  let mut bytes=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  let mut output=[0u8;CONTENT_MAX];
  let (next,n)=replace_file(&bytes[..size],old,new,&mut output)?;
  if n>0{result.files+=1;result.occurrences+=n;
   result.bytes_delta+=next as isize-size as isize;}
 }Ok(result)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn atomic_cross_file_replace_preserves_real_source_contents(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"foo foo").unwrap();
  fs.replace(b"/src/b",b"call foo").unwrap();
  assert_eq!(preview(&fs,b"/src",b"foo",b"bar").unwrap().occurrences,3);
  let r=replace_all::<2,1024>(&mut fs,2,b"/src",b"foo",b"bar").unwrap();
  assert_eq!(r.files,2);let mut out=[0;7];
  fs.read(b"/src/a",0,&mut out).unwrap();assert_eq!(&out,b"bar bar");
 }
 #[test]fn second_file_overgrowth_rolls_back_first_file_edit(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"x").unwrap();
  let mut many=[b' ';CONTENT_MAX];
  for p in (0..CONTENT_MAX).step_by(2){many[p]=b'x';}
  fs.replace(b"/src/b",&many).unwrap();
  assert_eq!(replace_all::<2,5000>(&mut fs,2,b"/src",b"x",b"LONG"),
   Err(Error::Full));
  let mut out=[0];fs.read(b"/src/a",0,&mut out).unwrap();assert_eq!(out,[b'x']);
 }
 #[test]fn invalid_empty_search_and_stale_revision_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"aa").unwrap();
  assert_eq!(replace_all::<1,512>(&mut fs,0,b"/",b"a",b"b"),Err(Error::Stale));
  assert_eq!(replace_all::<1,512>(&mut fs,1,b"/",b"",b"b"),Err(Error::Invalid));
 }
}
