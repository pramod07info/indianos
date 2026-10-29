//! R030/02: atomic multi-file recycle bin using real source file generations.
//! Stored under /trash/<original-path>; these RAM files are not durable disk.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub fn recycle<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 version:u64,files:&[&[u8]])->Result<usize,Error>{
 if fs.version()!=version{return Err(Error::Stale);}
 if files.is_empty()||files.len()>F{return Err(Error::Invalid);}
 let mut stage_names=[[0u8;PATH_MAX];F];
 let mut revs=[0u64;F];
 for(i,&source) in files.iter().enumerate(){
  if !source.starts_with(b"/")||source.starts_with(b"/trash/")||
   source.len()+6>PATH_MAX||source.len()<2||
   files[..i].contains(&source){return Err(Error::Invalid);}
  stage_names[i][..6].copy_from_slice(b"/trash");
  stage_names[i][6..6+source.len()].copy_from_slice(source);
  if fs.metadata(&stage_names[i][..6+source.len()]).is_ok(){
   return Err(Error::Conflict);
  }
  revs[i]=fs.metadata(source).map_err(|_|Error::Missing)?.0;
 }
 let mut raw=[0u8;B];let n=fs.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();staged.import_snapshot(&raw[..n])
  .map_err(|_|Error::Invalid)?;
 for(i,&source) in files.iter().enumerate(){
  staged.rename_if_version(source,&stage_names[i][..6+source.len()],revs[i])
   .map_err(|_|Error::Conflict)?;
 }
 let n=staged.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 fs.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;Ok(files.len())
}
pub fn recover<const F:usize>(fs:&mut RamFs<F>,version:u64,
 source:&[u8])->Result<u64,Error>{
 if fs.version()!=version{return Err(Error::Stale);}
 if !source.starts_with(b"/")||source.len()+6>PATH_MAX||
 fs.metadata(source).is_ok(){return Err(Error::Conflict);}
 let mut path=[0u8;PATH_MAX];path[..6].copy_from_slice(b"/trash");
 path[6..6+source.len()].copy_from_slice(source);
 let (rev,_)=fs.metadata(&path[..6+source.len()]).map_err(|_|Error::Missing)?;
 fs.rename_if_version(&path[..6+source.len()],source,rev).map_err(|_|Error::Conflict)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn multiple_guest_files_move_to_trash_and_recover(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"one").unwrap();
  fs.replace(b"/p/b",b"two").unwrap();
  assert_eq!(recycle::<2,2048>(&mut fs,2,&[b"/p/a",b"/p/b"]),Ok(2));
  assert!(fs.metadata(b"/trash/p/b").is_ok());
  recover(&mut fs,fs.version(),b"/p/a").unwrap();
  let mut out=[0;3];fs.read(b"/p/a",0,&mut out).unwrap();assert_eq!(&out,b"one");
 }
 #[test]fn existing_trash_collision_blocks_entire_recycle(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();
  fs.replace(b"/trash/p/a",b"old").unwrap();
  assert_eq!(recycle::<2,2048>(&mut fs,2,&[b"/p/a"]),Err(Error::Conflict));
  assert!(fs.metadata(b"/p/a").is_ok());
 }
}