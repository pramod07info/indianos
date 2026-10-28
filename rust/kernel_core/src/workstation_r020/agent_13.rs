//! R020/13 guarded source patch application on actual RAM-backed project files.
//! An exact old-content preimage and filesystem revision are mandatory.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct Patch<'a>{pub path:&'a[u8],pub offset:usize,
 pub old:&'a[u8],pub replacement:&'a[u8]}
pub fn apply<const F:usize,const IMAGE:usize>(fs:&mut RamFs<F>,
 rev:u64,patches:&[Patch<'_>])->Result<u64,Error>{
 if patches.is_empty()||rev!=fs.version(){return Err(Error::Stale);}
 for (i,p) in patches.iter().enumerate(){
  if p.old.is_empty()||patches[..i].iter().any(|old|old.path==p.path){
   return Err(Error::Conflict);
  }
 }
 if patches.len()>F{return Err(Error::Full);}
 let mut snapshot=[0u8;IMAGE];
 let size=fs.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&snapshot[..size]).map_err(|_|Error::Invalid)?;
 for (index,p) in patches.iter().enumerate(){
  if p.old.is_empty()||patches[..index].iter().any(|x|x.path==p.path){
   return Err(Error::Conflict);
  }
  let mut data=[0u8;CONTENT_MAX];
  let (_,length)=staged.metadata(p.path).map_err(|_|Error::Missing)?;
  if staged.read(p.path,0,&mut data[..length]).map_err(|_|Error::Missing)?!=length{
   return Err(Error::Stale);
  }
  let end=p.offset.checked_add(p.old.len()).ok_or(Error::Overflow)?;
  if end>length||&data[p.offset..end]!=p.old{return Err(Error::Stale);}
  let newlen=length-p.old.len()+p.replacement.len();
  if newlen>CONTENT_MAX{return Err(Error::Full);}
  let mut updated=[0u8;CONTENT_MAX];
  updated[..p.offset].copy_from_slice(&data[..p.offset]);
  updated[p.offset..p.offset+p.replacement.len()].copy_from_slice(p.replacement);
  updated[p.offset+p.replacement.len()..newlen].copy_from_slice(&data[end..length]);
  staged.replace(p.path,&updated[..newlen]).map_err(|_|Error::Full)?;
 }
 let mut image=[0u8;IMAGE];
 let len=staged.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&image[..len]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn can_apply<const F:usize>(fs:&RamFs<F>,patch:&Patch<'_>)->bool{
 let Ok((_,size))=fs.metadata(patch.path)else{return false;};
 let Some(end)=patch.offset.checked_add(patch.old.len())else{return false;};
 if end>size||patch.old.is_empty(){return false;}
 let mut old=[0u8;CONTENT_MAX];
 fs.read(patch.path,patch.offset as u64,&mut old[..patch.old.len()])
   .is_ok_and(|n|n==patch.old.len()&&&old[..n]==patch.old)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn applies_two_source_file_hunks_in_one_snapshot_commit(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"fn foo").unwrap();fs.replace(b"/b",b"call foo").unwrap();
  let list=[Patch{path:b"/a",offset:3,old:b"foo",replacement:b"bar"},
   Patch{path:b"/b",offset:5,old:b"foo",replacement:b"bar"}];
  assert!(can_apply(&fs,&list[0]));
  apply::<2,2048>(&mut fs,2,&list).unwrap();
  let mut a=[0u8;6];fs.read(b"/a",0,&mut a).unwrap();assert_eq!(&a,b"fn bar");
 }
 #[test]fn stale_second_patch_leaves_first_source_file_untouched(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"abc").unwrap();fs.replace(b"/b",b"def").unwrap();
  let list=[Patch{path:b"/a",offset:0,old:b"a",replacement:b"x"},
   Patch{path:b"/b",offset:0,old:b"x",replacement:b"z"}];
  assert_eq!(apply::<2,2048>(&mut fs,2,&list),Err(Error::Stale));
  let mut out=[0u8;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"abc");
 }
 #[test]fn repeat_patch_same_file_refuses_ambiguous_overlap(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abc").unwrap();
  let list=[Patch{path:b"/a",offset:0,old:b"a",replacement:b"x"},
   Patch{path:b"/a",offset:1,old:b"b",replacement:b"y"}];
  assert_eq!(apply::<1,1024>(&mut fs,1,&list),Err(Error::Conflict));
 }
}
