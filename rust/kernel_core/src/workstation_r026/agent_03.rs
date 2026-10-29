//! R026/03: one-commit multi-file project copy using RamFs staging snapshot.
//! All names and source revisions preflight before publishing the target tree.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy)]
pub struct Copy<'a>{pub from:&'a[u8],pub to:&'a[u8],pub revision:u64}
pub fn batch<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 expected:u64,operations:&[Copy<'_>])->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if operations.is_empty()||operations.len()>F{return Err(Error::Invalid);}
 for (i,op) in operations.iter().enumerate(){
  if op.from.len()<2||op.to.len()<2||op.from.len()>PATH_MAX||
   op.to.len()>PATH_MAX||op.from[0]!=b'/'||op.to[0]!=b'/'||
   op.from.windows(2).any(|w|w==b"..")||
   op.to.windows(2).any(|w|w==b"..")||
   op.from==op.to||operations[..i].iter().any(|old|old.to==op.to){
   return Err(Error::Invalid);
  }
  if fs.metadata(op.from).map_err(|_|Error::Missing)?.0!=op.revision{
   return Err(Error::Stale);
  }
  if fs.metadata(op.to).is_ok(){return Err(Error::Conflict);}
 }
 let mut source=[0u8;B];
 let n=fs.export_snapshot(&mut source).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&source[..n]).map_err(|_|Error::Invalid)?;
 for op in operations{
  staged.copy_new(op.from,op.to).map_err(|_|Error::Full)?;
 }
 let mut result=[0u8;B];
 let n=staged.export_snapshot(&mut result).map_err(|_|Error::Full)?;
 fs.import_snapshot(&result[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn projected_files<const F:usize>(fs:&RamFs<F>,ops:&[Copy<'_>])
 ->Result<usize,Error>{
 fs.count().checked_add(ops.len()).filter(|&n|n<=F).ok_or(Error::Full)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn copies_two_real_project_files_in_one_atomic_snapshot(){
  let mut fs=RamFs::<4>::new();
  fs.replace(b"/a",b"A").unwrap();fs.replace(b"/b",b"B").unwrap();
  let ops=[Copy{from:b"/a",to:b"/x",revision:1},
   Copy{from:b"/b",to:b"/y",revision:2}];
  batch::<4,2048>(&mut fs,2,&ops).unwrap();
  let mut out=[0;1];fs.read(b"/y",0,&mut out).unwrap();assert_eq!(&out,b"B");
 }
 #[test]fn later_collision_rolls_back_earlier_file_copy(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  fs.replace(b"/z",b"z").unwrap();
  let ops=[Copy{from:b"/a",to:b"/x",revision:1},
   Copy{from:b"/b",to:b"/z",revision:2}];
  assert_eq!(batch::<3,2048>(&mut fs,3,&ops),Err(Error::Conflict));
  assert!(fs.metadata(b"/x").is_err());
 }
 #[test]fn stale_source_revisions_reject_entire_copy_transaction(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  let ops=[Copy{from:b"/a",to:b"/x",revision:1}];
  assert_eq!(batch::<2,2048>(&mut fs,2,&ops),Err(Error::Stale));
 }
}
