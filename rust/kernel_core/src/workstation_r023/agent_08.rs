//! R023/08: real-file three-way textual merge preview with explicit conflicts.
//! No heuristic auto-merge for simultaneous divergent edits of the same range.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Decision{Unchanged,Ours,Theirs,Identical,Conflict}
pub fn classify(base:&[u8],ours:&[u8],theirs:&[u8])->Decision{
 if ours==base&&theirs==base{Decision::Unchanged}
 else if ours==theirs{Decision::Identical}
 else if ours==base{Decision::Theirs}
 else if theirs==base{Decision::Ours}
 else{Decision::Conflict}
}
pub fn three_way<const B:usize>(base:&[u8],ours:&[u8],
 theirs:&[u8],out:&mut[u8;B])->Result<(usize,Decision),Error>{
 let decision=classify(base,ours,theirs);
 if decision==Decision::Conflict{return Err(Error::Conflict);}
 let source=match decision{
  Decision::Unchanged=>base,
  Decision::Ours=>ours,
  Decision::Theirs=>theirs,
  Decision::Identical=>ours,
  Decision::Conflict=>return Err(Error::Conflict)
 };
 if source.len()>B{return Err(Error::Full);}
 let mut stage=[0u8;B];stage[..source.len()].copy_from_slice(source);
 *out=stage;Ok((source.len(),decision))
}
pub fn file_merge<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected_file_version:u64,base:&[u8],ours:&[u8],
 theirs:&[u8])->Result<(u64,Decision),Error>{
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=expected_file_version{return Err(Error::Stale);}
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..size]).map_err(|_|Error::Missing)?;
 if &data[..size]!=ours{return Err(Error::Stale);}
 let mut merged=[0u8;CONTENT_MAX];
 let (n,choice)=three_way(base,ours,theirs,&mut merged)?;
 if merged[..n]==data[..size]{return Ok((version,choice));}
 let new_version=fs.replace(path,&merged[..n]).map_err(|_|Error::Full)?;
 Ok((new_version,choice))
}
pub fn common_prefix(a:&[u8],b:&[u8])->usize{
 a.iter().zip(b).take_while(|(a,b)|a==b).count()
}
pub fn common_suffix(a:&[u8],b:&[u8])->usize{
 a.iter().rev().zip(b.iter().rev()).take_while(|(a,b)|a==b).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn upstream_only_edits_fast_forward_real_project_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"original").unwrap();
  assert_eq!(file_merge(&mut fs,b"/a",1,b"original",b"original",b"upstream"),
   Ok((2,Decision::Theirs)));
  let mut out=[0;8];fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out,b"upstream");
 }
 #[test]fn divergent_concurrent_edits_never_erase_either_user_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"ours").unwrap();
  assert_eq!(file_merge(&mut fs,b"/a",1,b"base",b"ours",b"theirs"),
   Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
 #[test]fn truncated_merge_output_is_fail_atomic(){
  let mut out=[44u8;2];
  assert_eq!(three_way(b"base",b"base",b"upstream",&mut out),
   Err(Error::Full));assert_eq!(out,[44,44]);
  assert_eq!(common_prefix(b"hello",b"help"),3);
 }
}
