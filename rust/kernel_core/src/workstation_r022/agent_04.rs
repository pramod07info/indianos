//! R022/04: literal search across actual workspace source files.
//! Atomic result buffer, sorted file traversal, owner-side permission enforced by caller.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub path:[u8;PATH_MAX],pub len:usize,
 pub offset:usize,pub line:usize,pub revision:u64}
pub const EMPTY:Hit=Hit{path:[0;PATH_MAX],len:0,offset:0,line:0,revision:0};
pub fn search<const F:usize,const H:usize>(
 fs:&RamFs<F>,prefix:&[u8],needle:&[u8],hits:&mut[Hit;H])
 ->Result<usize,Error>{
 if needle.is_empty()||needle.len()>128||prefix.first()!=Some(&b'/'){
  return Err(Error::Invalid);
 }
 let mut staged=[EMPTY;H];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix){continue;}
  let mut data=[0u8;CONTENT_MAX];
  let n=fs.read(&path[..len],0,&mut data[..size])
   .map_err(|_|Error::Missing)?;
  if needle.len()>n{continue;}
  let mut line=1usize;
  for pos in 0..=n-needle.len(){
   if &data[pos..pos+needle.len()]==needle{
    if count==H{return Err(Error::Full);}
    staged[count]=Hit{path,len,offset:pos,line,revision};count+=1;
   }
   if data[pos]==b'\n'{line+=1;}
  }
 }
 staged[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len])
   .then(a.offset.cmp(&b.offset)));
 *hits=staged;Ok(count)
}
pub fn matches_file(hits:&[Hit],path:&[u8])->usize{
 hits.iter().filter(|h|&h.path[..h.len]==path).count()
}
pub fn first_after<'a>(hits:&'a[Hit],offset:usize)->Option<&'a Hit>{
 hits.iter().filter(|h|h.offset>=offset).min_by_key(|h|h.offset)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn search_returns_file_offsets_and_line_numbers_from_real_source(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a.rs",b"fn main\nmain").unwrap();
  fs.replace(b"/src/b.rs",b"main").unwrap();
  let mut hits=[EMPTY;5];
  assert_eq!(search(&fs,b"/src",b"main",&mut hits),Ok(3));
  assert_eq!(hits[1].line,2);assert_eq!(matches_file(&hits[..3],b"/src/a.rs"),2);
 }
 #[test]fn overflow_does_not_leak_partial_search_results(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/a",b"aaa").unwrap();
  let marker=Hit{offset:99,..EMPTY};let mut hits=[marker;1];
  assert_eq!(search(&fs,b"/src",b"a",&mut hits),Err(Error::Full));
  assert_eq!(hits,[marker]);
 }
 #[test]fn missing_prefix_or_empty_query_refuses_search(){
  let fs=RamFs::<1>::new();let mut hits=[EMPTY;1];
  assert_eq!(search(&fs,b"/",b"",&mut hits),Err(Error::Invalid));
  assert_eq!(search(&fs,b"src",b"a",&mut hits),Err(Error::Invalid));
 }
}
