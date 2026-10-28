//! R022/06: per-file digest inventory and integrity re-verification of live RamFs.
//! FNV64 is non-cryptographic; never substitute for signed software verification.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub path:[u8;PATH_MAX],pub len:usize,pub size:usize,
 pub version:u64,pub digest:u64}
pub const EMPTY:Entry=Entry{path:[0;PATH_MAX],len:0,size:0,version:0,digest:0};
pub fn checksum(data:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &v in data{h=(h^(v as u64)).wrapping_mul(0x100000001b3);}h
}
pub fn scan<const F:usize,const O:usize>(
 fs:&RamFs<F>,out:&mut[Entry;O])->Result<usize,Error>{
 if fs.count()>O{return Err(Error::Full);}
 let mut stage=[EMPTY;O];let mut i=0;
 for idx in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,version)=fs.entry(idx,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  let mut contents=[0u8;CONTENT_MAX];
  if fs.read(&path[..len],0,&mut contents[..size])
    .map_err(|_|Error::Missing)?!=size{return Err(Error::Stale);}
  stage[i]=Entry{path,len,size,version,digest:checksum(&contents[..size])};
  i+=1;
 }
 stage[..i].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=stage;Ok(i)
}
pub fn verify<const F:usize>(fs:&RamFs<F>,entry:Entry)
 ->Result<(),Error>{
 if entry.len==0||entry.len>PATH_MAX{return Err(Error::Invalid);}
 let (version,size)=fs.metadata(&entry.path[..entry.len]).map_err(|_|Error::Missing)?;
 if version!=entry.version||size!=entry.size{return Err(Error::Stale);}
 let mut contents=[0u8;CONTENT_MAX];
 fs.read(&entry.path[..entry.len],0,&mut contents[..size])
  .map_err(|_|Error::Missing)?;
 if checksum(&contents[..size])!=entry.digest{return Err(Error::Conflict);}
 Ok(())
}
pub fn total_size(entries:&[Entry])->Result<usize,Error>{
 entries.iter().try_fold(0usize,|n,e|
  n.checked_add(e.size).ok_or(Error::Overflow))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_workspace_integrity_index_is_deterministic(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/z.rs",b"z").unwrap();fs.replace(b"/a.rs",b"alpha").unwrap();
  let mut out=[EMPTY;2];assert_eq!(scan(&fs,&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/a.rs");
  assert_eq!(total_size(&out),Ok(6));assert_eq!(verify(&fs,out[0]),Ok(()));
 }
 #[test]fn changed_file_fails_old_integrity_manifest(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut out=[EMPTY;1];scan(&fs,&mut out).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(verify(&fs,out[0]),Err(Error::Stale));
 }
 #[test]fn insufficient_output_does_not_clobber_integrity_snapshot(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"one").unwrap();fs.replace(b"/b",b"two").unwrap();
  let marked=Entry{digest:77,..EMPTY};let mut out=[marked;1];
  assert_eq!(scan(&fs,&mut out),Err(Error::Full));assert_eq!(out,[marked]);
 }
}
