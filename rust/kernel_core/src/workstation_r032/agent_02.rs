//! R032/02: ordered project file inventory with stable content hash and revisions.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub bytes:usize,pub checksum:u64}
pub const EMPTY:Entry=Entry{path:[0;PATH_MAX],len:0,revision:0,bytes:0,checksum:0};
pub fn digest(data:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in data{h=(h^(b as u64)).wrapping_mul(0x100000001b3);}h
}
pub fn inspect<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Entry;N])->Result<usize,Error>{
 if root.len()<2||root[0]!=b'/'||root.ends_with(b"/"){return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let (len,bytes,revision)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||path.get(root.len())!=Some(&b'/'){continue;}
  if n==N{return Err(Error::Full);}
  let mut data=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut data[..bytes])
   .map_err(|_|Error::Missing)?;
  stage[n]=Entry{path,len,revision,bytes,checksum:digest(&data[..bytes])};n+=1;
 }
 stage[..n].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));
 *out=stage;Ok(n)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,entry:Entry)->bool{
 if entry.len==0||entry.len>PATH_MAX{return false;}
 let path=&entry.path[..entry.len];let Ok((rev,n))=fs.metadata(path)else{return false;};
 if rev!=entry.revision||n!=entry.bytes||n>CONTENT_MAX{return false;}
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..n]).is_ok()&&digest(&data[..n])==entry.checksum
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn ordered_inventory_matches_actual_file_bytes(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/p/z",b"last").unwrap();fs.replace(b"/p/a",b"first").unwrap();
  let mut out=[EMPTY;2];assert_eq!(inspect(&fs,b"/p",&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/p/a");
  assert_eq!(out[0].checksum,digest(b"first"));assert!(valid(&fs,out[0]));
 }
 #[test]fn change_invalidates_manifest_and_overflow_does_not_publish(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut out=[EMPTY;1];inspect(&fs,b"/p",&mut out).unwrap();let old=out[0];
  fs.replace(b"/p/a",b"b").unwrap();assert!(!valid(&fs,old));
  fs.replace(b"/p/b",b"b").unwrap();
  assert_eq!(inspect(&fs,b"/p",&mut out),Err(Error::Full));assert_eq!(out[0],old);
 }
}