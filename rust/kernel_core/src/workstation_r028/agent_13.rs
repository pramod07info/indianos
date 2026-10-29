//! R028/13: source-content build cache invalidation from real Rust files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Input{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,pub digest:u64}
pub const EMPTY:Input=Input{path:[0;PATH_MAX],len:0,revision:0,digest:0};
fn hash(bytes:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
pub fn capture<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Input;N])->Result<usize,Error>{
 if root.first()!=Some(&b'/'){return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut count=0;
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let(len,size,revision)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !name[..len].starts_with(root)||!name[..len].ends_with(b".rs"){continue;}
  if count==N{return Err(Error::Full);}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(&name[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  stage[count]=Input{path:name,len,revision,digest:hash(&bytes[..size])};count+=1;
 }
 *out=stage;Ok(count)
}
pub fn dirty<const F:usize>(fs:&RamFs<F>,inputs:&[Input])->Result<usize,Error>{
 let mut changed=0;
 for entry in inputs{
  let path=&entry.path[..entry.len];
  let Ok((revision,size))=fs.metadata(path)else{changed+=1;continue};
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  if revision!=entry.revision||hash(&bytes[..size])!=entry.digest{changed+=1;}
 }Ok(changed)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_modified_source_requires_incremental_rebuild(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a.rs",b"a").unwrap();fs.replace(b"/p/b.rs",b"b").unwrap();
  let mut sources=[EMPTY;2];capture(&fs,b"/p",&mut sources).unwrap();
  fs.replace(b"/p/a.rs",b"new").unwrap();
  assert_eq!(dirty(&fs,&sources),Ok(1));
 }
 #[test]fn insufficient_source_inventory_space_does_not_publish_partial_cache(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a.rs",b"a").unwrap();fs.replace(b"/p/b.rs",b"b").unwrap();
  let mut out=[Input{revision:99,..EMPTY};1];
  assert_eq!(capture(&fs,b"/p",&mut out),Err(Error::Full));
  assert_eq!(out[0].revision,99);
 }
}