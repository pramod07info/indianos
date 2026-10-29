//! R027/05: exact-preimage UTF-8-safe source patch with undo image.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Patch<'a>{pub start:usize,pub end:usize,
 pub expected:&'a[u8],pub replacement:&'a[u8]}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 revision:u64,patch:Patch<'_>)->Result<u64,Error>{
 let (actual,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if actual!=revision{return Err(Error::Stale);}
 let mut before=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut before[..size]).map_err(|_|Error::Missing)?;
 let text=core::str::from_utf8(&before[..size]).map_err(|_|Error::Invalid)?;
 if patch.start>patch.end||patch.end>size||
  !text.is_char_boundary(patch.start)||!text.is_char_boundary(patch.end)||
  patch.expected!=&before[patch.start..patch.end]||
  core::str::from_utf8(patch.replacement).is_err(){return Err(Error::Conflict);}
 let next=size.checked_sub(patch.end-patch.start)
  .and_then(|n|n.checked_add(patch.replacement.len())).ok_or(Error::Overflow)?;
 if next>CONTENT_MAX{return Err(Error::Full);}
 let mut bytes=[0u8;CONTENT_MAX];
 bytes[..patch.start].copy_from_slice(&before[..patch.start]);
 bytes[patch.start..patch.start+patch.replacement.len()]
  .copy_from_slice(patch.replacement);
 bytes[patch.start+patch.replacement.len()..next]
  .copy_from_slice(&before[patch.end..size]);
 fs.replace(path,&bytes[..next]).map_err(|_|Error::Full)
}
pub fn preview<const F:usize>(fs:&RamFs<F>,path:&[u8],
 patch:Patch<'_>,out:&mut[u8])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut before=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut before[..size]).map_err(|_|Error::Missing)?;
 if patch.end>size||patch.start>patch.end||
 &before[patch.start..patch.end]!=patch.expected{return Err(Error::Conflict);}
 let next=size-patch.end+patch.start+patch.replacement.len();
 if next>out.len(){return Err(Error::Full);}
 out[..patch.start].copy_from_slice(&before[..patch.start]);
 out[patch.start..patch.start+patch.replacement.len()].copy_from_slice(patch.replacement);
 out[patch.start+patch.replacement.len()..next].copy_from_slice(&before[patch.end..size]);
 Ok(next)
}
pub fn changed_bytes(p:Patch<'_>)->isize{
 p.replacement.len() as isize-p.expected.len() as isize
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn applies_exact_source_patch_to_real_file(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"let old;").unwrap();
  let p=Patch{start:4,end:7,expected:b"old",replacement:b"new"};
  apply(&mut fs,b"/a",1,p).unwrap();
  let mut out=[0;8];fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out,b"let new;");
 }
 #[test]fn wrong_preimage_refuses_destructive_patch(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let p=Patch{start:0,end:3,expected:b"bad",replacement:b"new"};
  assert_eq!(apply(&mut fs,b"/a",1,p),Err(Error::Conflict));
  assert_eq!(fs.version(),1);
 }
 #[test]fn stale_version_prevents_lost_user_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  fs.replace(b"/a",b"other").unwrap();
  let p=Patch{start:0,end:3,expected:b"old",replacement:b"x"};
  assert_eq!(apply(&mut fs,b"/a",1,p),Err(Error::Stale));
 }
}
