//! R030/03: collision-free atomic batch rename with live file revision checks.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy)]
pub struct Rename<'a>{pub from:&'a[u8],pub to:&'a[u8],pub revision:u64}
pub fn apply<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 expected:u64,items:&[Rename<'_>])->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if items.is_empty()||items.len()>F{return Err(Error::Invalid);}
 for (i,item) in items.iter().enumerate(){
  if item.from.len()<2||item.to.len()<2||item.from.len()>PATH_MAX||
   item.to.len()>PATH_MAX||item.from==item.to||
   !item.from.starts_with(b"/")||!item.to.starts_with(b"/")||
   items[..i].iter().any(|v|v.from==item.from||v.to==item.to){
   return Err(Error::Invalid);
  }
  if fs.metadata(item.from).map_err(|_|Error::Missing)?.0!=item.revision{
   return Err(Error::Stale);
  }
  if fs.metadata(item.to).is_ok(){return Err(Error::Conflict);}
 }
 let mut data=[0u8;B];let n=fs.export_snapshot(&mut data).map_err(|_|Error::Full)?;
 let mut next=RamFs::<F>::new();
 next.import_snapshot(&data[..n]).map_err(|_|Error::Invalid)?;
 for item in items{
  next.rename_if_version(item.from,item.to,item.revision)
   .map_err(|_|Error::Conflict)?;
 }
 let n=next.export_snapshot(&mut data).map_err(|_|Error::Full)?;
 fs.import_snapshot(&data[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preview<const F:usize>(fs:&RamFs<F>,items:&[Rename<'_>])->Result<usize,Error>{
 for i in items{
  if fs.metadata(i.from).map_err(|_|Error::Missing)?.0!=i.revision{
   return Err(Error::Stale);
  }
  if fs.metadata(i.to).is_ok(){return Err(Error::Conflict);}
 }Ok(items.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn simultaneous_source_renames_retain_real_bytes(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"A").unwrap();
  fs.replace(b"/b",b"B").unwrap();
  let change=[Rename{from:b"/a",to:b"/c",revision:1},
   Rename{from:b"/b",to:b"/d",revision:2}];
  apply::<2,1024>(&mut fs,2,&change).unwrap();
  let mut data=[0;1];fs.read(b"/d",0,&mut data).unwrap();assert_eq!(&data,b"B");
 }
 #[test]fn late_destination_conflict_preserves_original_paths(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/a",b"A").unwrap();
  fs.replace(b"/b",b"B").unwrap();fs.replace(b"/z",b"Z").unwrap();
  let items=[Rename{from:b"/a",to:b"/new",revision:1},
   Rename{from:b"/b",to:b"/z",revision:2}];
  assert_eq!(apply::<3,1024>(&mut fs,3,&items),Err(Error::Conflict));
  assert!(fs.metadata(b"/a").is_ok());
 }
}