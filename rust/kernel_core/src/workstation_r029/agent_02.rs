//! R029/02: conservative three-way source merge using real file bytes.
//! Never silently merges conflicting concurrent edits; full semantic merge is future work.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Choice{Current,Incoming,Identical,Conflict}
pub fn classify(base:&[u8],current:&[u8],incoming:&[u8])->Choice{
 if current==incoming{Choice::Identical}
 else if current==base{Choice::Incoming}
 else if incoming==base{Choice::Current}
 else{Choice::Conflict}
}
pub fn merge<const F:usize>(fs:&mut RamFs<F>,path:&[u8],revision:u64,
 base:&[u8],incoming:&[u8])->Result<(Choice,u64),Error>{
 let (actual,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if revision!=actual{return Err(Error::Stale);}
 if incoming.len()>CONTENT_MAX||base.len()>CONTENT_MAX{return Err(Error::Full);}
 let mut current=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut current[..size]).map_err(|_|Error::Missing)?;
 let decision=classify(base,&current[..size],incoming);
 match decision{
  Choice::Conflict=>Err(Error::Conflict),
  Choice::Current|Choice::Identical=>Ok((decision,fs.version())),
  Choice::Incoming=>fs.replace(path,incoming).map(|v|(decision,v)).map_err(|_|Error::Full)
 }
}
pub fn conflict_count<'a>(items:&[(&'a[u8],&'a[u8],&'a[u8])])->usize{
 items.iter().filter(|(base,ours,theirs)|classify(base,ours,theirs)==Choice::Conflict).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn merge_uses_incoming_only_when_local_file_matches_base(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"base").unwrap();
  assert_eq!(merge(&mut fs,b"/a.rs",1,b"base",b"incoming").unwrap().0,Choice::Incoming);
  let mut out=[0;8];fs.read(b"/a.rs",0,&mut out).unwrap();
  assert_eq!(&out,b"incoming");
 }
 #[test]fn diverged_source_preserved_on_conflict(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"local").unwrap();
  assert_eq!(merge(&mut fs,b"/a",1,b"base",b"remote"),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
}