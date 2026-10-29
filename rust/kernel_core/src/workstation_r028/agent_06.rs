//! R028/06: multi-file editor patches committed atomically to real RamFs.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r027::agent_05 as patch;
pub struct Edit<'a>{pub path:&'a[u8],pub revision:u64,pub start:usize,
 pub end:usize,pub old:&'a[u8],pub new:&'a[u8]}
pub fn commit<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 version:u64,edits:&[Edit<'_>])->Result<u64,Error>{
 if version!=fs.version(){return Err(Error::Stale);}
 if edits.is_empty()||edits.len()>F{return Err(Error::Invalid);}
 for(i,e) in edits.iter().enumerate(){
  if e.path.len()<2||e.path.len()>PATH_MAX||
   edits[..i].iter().any(|prior|prior.path==e.path){return Err(Error::Invalid);}
  if fs.metadata(e.path).map_err(|_|Error::Missing)?.0!=e.revision{
   return Err(Error::Stale);
  }
 }
 let mut original=[0u8;B];
 let n=fs.export_snapshot(&mut original).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&original[..n]).map_err(|_|Error::Invalid)?;
 for e in edits{
  patch::apply(&mut staged,e.path,e.revision,
   patch::Patch{start:e.start,end:e.end,expected:e.old,replacement:e.new})
   .map_err(|_|Error::Conflict)?;
 }
 let mut output=[0u8;B];
 let len=staged.export_snapshot(&mut output).map_err(|_|Error::Full)?;
 fs.import_snapshot(&output[..len]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn affected(edits:&[Edit<'_>])->usize{edits.len()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn editing_two_real_source_files_is_atomic(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"old").unwrap();fs.replace(b"/b",b"old").unwrap();
  let edits=[Edit{path:b"/a",revision:1,start:0,end:3,old:b"old",new:b"A"},
   Edit{path:b"/b",revision:2,start:0,end:3,old:b"old",new:b"B"}];
  commit::<2,2048>(&mut fs,2,&edits).unwrap();
  let mut out=[0u8;1];fs.read(b"/b",0,&mut out).unwrap();assert_eq!(&out,b"B");
 }
 #[test]fn invalid_late_patch_rolls_back_all_edits(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"old").unwrap();fs.replace(b"/b",b"old").unwrap();
  let edits=[Edit{path:b"/a",revision:1,start:0,end:3,old:b"old",new:b"A"},
   Edit{path:b"/b",revision:2,start:0,end:3,old:b"bad",new:b"B"}];
  assert_eq!(commit::<2,2048>(&mut fs,2,&edits),Err(Error::Conflict));
  let mut out=[0u8;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"old");
 }
}