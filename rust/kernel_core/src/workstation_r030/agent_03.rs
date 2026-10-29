//! R030/03: revision-checked, collision-safe copy of a live source file.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn copy<const F:usize>(fs:&mut RamFs<F>,root:&[u8],
 source:&[u8],target:&[u8],expected:u64)->Result<u64,Error>{
 if root.len()<2||source==target||
  !source.starts_with(root)||source.get(root.len())!=Some(&b'/')||
  !target.starts_with(root)||target.get(root.len())!=Some(&b'/'){
  return Err(Error::Denied);
 }
 let (rev,len)=fs.metadata(source).map_err(|_|Error::Missing)?;
 if rev!=expected{return Err(Error::Stale);}
 if fs.metadata(target).is_ok(){return Err(Error::Conflict);}
 let mut bytes=[0u8;CONTENT_MAX];
 let n=fs.read(source,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
 fs.copy_new(source,target).map_err(|_|Error::Full).and_then(|new_rev|{
  if n!=len{return Err(Error::Conflict);}Ok(new_rev)
 })
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn copy_stages_real_source_without_overwriting_original(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"source").unwrap();
  assert_eq!(copy(&mut fs,b"/p",b"/p/a",b"/p/b",1),Ok(2));
  let mut bytes=[0;6];fs.read(b"/p/b",0,&mut bytes).unwrap();
  assert_eq!(&bytes,b"source");
 }
 #[test]fn stale_copy_and_cross_project_destination_are_denied(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"code").unwrap();
  assert_eq!(copy(&mut fs,b"/p",b"/p/a",b"/p/b",0),Err(Error::Stale));
  assert_eq!(copy(&mut fs,b"/p",b"/p/a",b"/p2/b",1),Err(Error::Denied));
  assert_eq!(fs.count(),1);
 }
}