//! R030/14: stable fingerprint of real source bytes and revisions for build cache.
use super::{Error,agent_01};
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
fn mix(mut h:u64,bytes:&[u8])->u64{
 for &b in bytes{h=(h^(b as u64)).wrapping_mul(0x100000001b3);}h
}
pub fn fingerprint<const F:usize>(fs:&RamFs<F>,root:&[u8],
 toolchain:&[u8])->Result<(u64,usize),Error>{
 if toolchain.is_empty()||toolchain.len()>128{return Err(Error::Invalid);}
 let mut files=[agent_01::EMPTY;F];let n=agent_01::inventory(fs,root,&mut files)?;
 let mut hash=mix(0xcbf29ce484222325,toolchain);let mut source_count=0;
 for item in &files[..n]{
  let name=&item.path[..item.len];
  if !name.ends_with(b".rs"){continue;}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(name,0,&mut bytes[..item.bytes]).map_err(|_|Error::Missing)?;
  hash=mix(hash,name);hash=mix(hash,&item.revision.to_le_bytes());
  hash=mix(hash,&bytes[..item.bytes]);source_count+=1;
 }
 Ok((hash,source_count))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn fingerprint_changes_when_real_source_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn a(){}").unwrap();
  let a=fingerprint(&fs,b"/p",b"rustc").unwrap();assert_eq!(a.1,1);
  fs.replace(b"/p/a.rs",b"fn b(){}").unwrap();
  assert_ne!(a.0,fingerprint(&fs,b"/p",b"rustc").unwrap().0);
 }
 #[test]fn non_source_files_are_excluded_but_toolchain_is_hashed(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.txt",b"hello").unwrap();
  let a=fingerprint(&fs,b"/p",b"1").unwrap();
  assert_eq!(a.1,0);assert_ne!(a.0,fingerprint(&fs,b"/p",b"2").unwrap().0);
 }
}