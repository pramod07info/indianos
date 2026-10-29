//! R027/04: source-integrity scan over actual guest files with stale detection.
//! Checksums are FNV-1a integrity hints, not cryptographic signatures.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Seal{pub path:[u8;PATH_MAX],pub len:usize,
 pub revision:u64,pub size:usize,pub checksum:u64}
pub fn hash(data:&[u8])->u64{
 let mut h=0xcbf29ce484222325u64;
 for &b in data{h=(h^(b as u64)).wrapping_mul(0x100000001b);}h
}
pub fn capture<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<Seal,Error>{
 if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
 let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut content=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut content[..size]).map_err(|_|Error::Missing)?;
 let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
 Ok(Seal{path:name,len:path.len(),revision,size,checksum:hash(&content[..size])})
}
pub fn verify<const F:usize>(fs:&RamFs<F>,seal:Seal)->Result<(),Error>{
 let path=&seal.path[..seal.len];
 let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=seal.revision||size!=seal.size{return Err(Error::Stale);}
 let mut bytes=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
 if hash(&bytes[..size])!=seal.checksum{return Err(Error::Conflict);}
 Ok(())
}
pub fn verify_all<const F:usize>(fs:&RamFs<F>,seals:&[Seal])
 ->Result<usize,Error>{
 for &seal in seals{verify(fs,seal)?;}Ok(seals.len())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn integrity_seal_matches_real_unchanged_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main.rs",b"fn x(){}").unwrap();
  let s=capture(&fs,b"/main.rs").unwrap();
  assert_eq!(verify_all(&fs,&[s]),Ok(1));
 }
 #[test]fn source_edit_invalidates_old_content_integrity(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let seal=capture(&fs,b"/a").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(verify(&fs,seal),Err(Error::Stale));
 }
 #[test]fn missing_file_does_not_verify_past_integrity_proof(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let seal=capture(&fs,b"/a").unwrap();
  fs.delete_if_version(b"/a",1).unwrap();
  assert_eq!(verify(&fs,seal),Err(Error::Missing));
 }
}
