//! R030/10: display source-editor conflicting revisions and safe three-way resolution.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Resolution{KeepDisk,ApplyDraft,NoChange,Conflict}
pub fn resolve(base:&[u8],disk:&[u8],draft:&[u8])->Resolution{
 if disk==draft{Resolution::NoChange}
 else if disk==base{Resolution::ApplyDraft}
 else if draft==base{Resolution::KeepDisk}
 else{Resolution::Conflict}
}
pub fn preview<const F:usize>(fs:&RamFs<F>,path:&[u8],
 original:&[u8],draft:&[u8])->Result<Resolution,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut disk=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut disk[..size]).map_err(|_|Error::Missing)?;
 if draft.len()>CONTENT_MAX{return Err(Error::Full);}
 Ok(resolve(original,&disk[..size],draft))
}
pub fn publish<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected:u64,base:&[u8],draft:&[u8])->Result<(Resolution,u64),Error>{
 let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=expected{return Err(Error::Stale);}
 let mut disk=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut disk[..size]).map_err(|_|Error::Missing)?;
 match resolve(base,&disk[..size],draft){
  Resolution::ApplyDraft=>fs.replace(path,draft).map(|v|(Resolution::ApplyDraft,v))
   .map_err(|_|Error::Full),
  Resolution::Conflict=>Err(Error::Conflict),
  other=>Ok((other,fs.version()))
 }
}
pub fn changed(base:&[u8],candidate:&[u8])->bool{base!=candidate}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn draft_publishes_only_when_disk_matches_original_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"original").unwrap();
  assert_eq!(preview(&fs,b"/a",b"original",b"edited"),Ok(Resolution::ApplyDraft));
  publish(&mut fs,b"/a",1,b"original",b"edited").unwrap();
  let mut out=[0;6];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"edited");
 }
 #[test]fn simultaneous_disk_and_draft_changes_require_manual_conflict_resolution(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"disk").unwrap();
  assert_eq!(publish(&mut fs,b"/a",1,b"base",b"draft"),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
}