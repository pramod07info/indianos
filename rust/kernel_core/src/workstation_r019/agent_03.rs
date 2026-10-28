//! R019/03: bounded cross-file literal search of the actual RAM project.
//! Search results have verified path and offsets, with no heap or regex engine.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub path:[u8;PATH_MAX],pub len:usize,pub offset:usize}
pub const EMPTY:Hit=Hit{path:[0;PATH_MAX],len:0,offset:0};
pub fn scan<const F:usize,const H:usize>(fs:&RamFs<F>,needle:&[u8],
 out:&mut[Hit;H])->Result<usize,Error>{
 if needle.is_empty()||needle.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let mut stage=[EMPTY;H];let mut count=0;
 for index in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,bytes,_) = fs.entry(index,&mut path).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
  if bytes>CONTENT_MAX{return Err(Error::Full);}
  let mut data=[0u8;CONTENT_MAX];
  if fs.read(&path[..len],0,&mut data[..bytes]).map_err(|_|Error::Missing)?!=bytes{
   return Err(Error::Stale);
  }
  for (offset,window) in data[..bytes].windows(needle.len()).enumerate(){
   if window==needle{
    if count>=H{return Err(Error::Full);}
    stage[count]=Hit{path,len,offset};count+=1;
   }
  }
 }
 *out=stage;Ok(count)
}
pub fn count_matches<const F:usize>(fs:&RamFs<F>,needle:&[u8])
 ->Result<usize,Error>{
 if needle.is_empty(){return Err(Error::Invalid);}
 let mut count=0;
 for index in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,bytes,_) = fs.entry(index,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  let mut data=[0u8;CONTENT_MAX];
  if fs.read(&path[..len],0,&mut data[..bytes]).map_err(|_|Error::Missing)?!=bytes{
   return Err(Error::Stale);
  }
  count+=data[..bytes].windows(needle.len()).filter(|w|*w==needle).count();
 }
 Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn literal_matches_multiple_source_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a.rs",b"let x; let y;").unwrap();fs.replace(b"/b.rs",b"let z;").unwrap();
  let mut out=[EMPTY;4];
  assert_eq!(scan(&fs,b"let",&mut out),Ok(3));
  assert_eq!(&out[0].path[..out[0].len],b"/a.rs");
  assert_eq!(&out[2].path[..out[2].len],b"/b.rs");
  assert_eq!(count_matches(&fs,b"let"),Ok(3));
 }
 #[test]fn overflow_results_do_not_publish_partial_hits(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"aaa").unwrap();
  let sentinel=Hit{offset:99,..EMPTY};let mut out=[sentinel;2];
  assert_eq!(scan(&fs,b"a",&mut out),Err(Error::Full));
  assert_eq!(out,[sentinel;2]);
 }
 #[test]fn empty_search_does_not_match_every_byte(){
  let fs=RamFs::<1>::new();let mut out=[EMPTY;2];
  assert_eq!(scan(&fs,b"",&mut out),Err(Error::Invalid));
 }
}
