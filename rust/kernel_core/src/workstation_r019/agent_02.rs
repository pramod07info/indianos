//! R019/02: multi-step editor, filesystem and checksum integration.
//! The in-memory editor does not become a native arbitrary-code executor.
use super::Error;
use crate::workstation_r018::editor::Buffer;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
use crate::batch6_storage::AtomicFileBackend;
pub fn editor_to_disk<const B:usize,const F:usize>(editor:&Buffer<B>,
 fs:&mut RamFs<F>,owner:u32,expected_fs:u64,path:&[u8])
 ->Result<u64,Error>{
 if B>CONTENT_MAX{return Err(Error::Full);}
 let mut bytes=[0u8;B];
 let count=editor.copy(owner,0,editor.len(),&mut bytes)
  .map_err(|_|Error::Denied)?;
 fs.save_if_revision(path,&bytes[..count],expected_fs).map_err(|_|Error::Stale)
}
pub fn disk_to_editor<const B:usize,const F:usize>(editor:&mut Buffer<B>,
 fs:&mut RamFs<F>,owner:u32,expected_editor:u64,path:&[u8])
 ->Result<u64,Error>{
 let mut bytes=[0u8;B];
 let count=fs.read_all(path,&mut bytes).map_err(|_|Error::Full)?;
 editor.load(owner,expected_editor,&bytes[..count]).map_err(|_|Error::Stale)
}
pub fn replace_one<const B:usize>(editor:&mut Buffer<B>,
 owner:u32,revision:u64,find:&[u8],replacing:&[u8])
 ->Result<u64,Error>{
 if find.is_empty(){return Err(Error::Invalid);}
 if editor.revision()!=revision{return Err(Error::Stale);}
 let at=editor.find(owner,find).map_err(|_|Error::Denied)?
  .ok_or(Error::Missing)?;
 editor.replace(owner,revision,at,at+find.len(),replacing)
  .map_err(|_|Error::Full)
}
pub fn checksum<const B:usize>(editor:&Buffer<B>,owner:u32)
 ->Result<u32,Error>{
 let mut data=[0u8;B];let size=editor.copy(owner,0,editor.len(),&mut data)
  .map_err(|_|Error::Denied)?;
 let mut h=0x811c9dc5u32;
 for &b in &data[..size]{h=(h^(b as u32)).wrapping_mul(0x01000193);}
 Ok(h)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn edit_replace_save_and_open_roundtrip(){
  let mut e=Buffer::<128>::new(7);let mut fs=RamFs::<1>::new();
  e.load(7,0,b"hello world").unwrap();
  assert_eq!(replace_one(&mut e,7,1,b"world",b"IndianOS"),Ok(2));
  assert_eq!(editor_to_disk(&e,&mut fs,7,0,b"/main"),Ok(1));
  let mut loaded=Buffer::<128>::new(7);
  disk_to_editor(&mut loaded,&mut fs,7,0,b"/main").unwrap();
  let mut bytes=[0u8;32];
  assert_eq!(loaded.copy(7,0,14,&mut bytes),Ok(14));
  assert_eq!(&bytes[..14],b"hello IndianOS");
 }
 #[test]fn stale_disk_save_does_not_publish_partial_document(){
  let mut e=Buffer::<32>::new(7);let mut fs=RamFs::<1>::new();
  e.load(7,0,b"old").unwrap();fs.replace(b"/a",b"new").unwrap();
  assert_eq!(editor_to_disk(&e,&mut fs,7,0,b"/a"),Err(Error::Stale));
  let mut got=[0u8;3];fs.read(b"/a",0,&mut got).unwrap();assert_eq!(&got,b"new");
 }
 #[test]fn reused_owner_and missing_word_are_denied(){
  let mut e=Buffer::<32>::new(7);e.load(7,0,b"hello").unwrap();
  assert_eq!(replace_one(&mut e,8,1,b"hello",b"x"),Err(Error::Denied));
  assert_eq!(replace_one(&mut e,7,1,b"absent",b"x"),Err(Error::Missing));
  assert!(checksum(&e,8).is_err());
 }
}
