//! R029/01: recoverable project-file trash using an atomic guest RamFs transaction.
//! Trash names are explicitly scoped; contents remain volatile until durable VFS persists them.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub fn trash<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 source:&[u8],trash_path:&[u8])->Result<u64,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if source==trash_path||trash_path.len()>PATH_MAX||
 !trash_path.starts_with(b"/trash/"){return Err(Error::Invalid);}
 let (revision,_)=fs.metadata(source).map_err(|_|Error::Missing)?;
 if fs.metadata(trash_path).is_ok(){return Err(Error::Conflict);}
 let mut raw=[0u8;B];let n=fs.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();staged.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;
 staged.rename_if_version(source,trash_path,revision).map_err(|_|Error::Conflict)?;
 let n=staged.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 fs.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn restore<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 trash_path:&[u8],to:&[u8])->Result<u64,Error>{
 if !trash_path.starts_with(b"/trash/"){return Err(Error::Invalid);}
 if to.starts_with(b"/trash/"){return Err(Error::Invalid);}
 trash::<F,B>(fs,expected,to,trash_path).err().map_or_else(
 ||Err(Error::Conflict),|_|{
 if fs.version()!=expected{return Err(Error::Stale);}
 let (rev,_)=fs.metadata(trash_path).map_err(|_|Error::Missing)?;
 if fs.metadata(to).is_ok(){return Err(Error::Conflict);}
 fs.rename_if_version(trash_path,to,rev).map_err(|_|Error::Conflict)
 })
}
pub fn entries<const F:usize>(fs:&RamFs<F>)->usize{
 let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  if let Ok(Some((len,_,_)))=fs.entry(i,&mut path){
   if path[..len].starts_with(b"/trash/"){n+=1;}
  }
 }n
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn discarded_project_source_can_be_recovered(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/main.rs",b"source").unwrap();
  trash::<2,1024>(&mut fs,1,b"/p/main.rs",b"/trash/main.rs").unwrap();
  assert!(fs.metadata(b"/p/main.rs").is_err());assert_eq!(entries(&fs),1);
  restore::<2,1024>(&mut fs,2,b"/trash/main.rs",b"/p/main.rs").unwrap();
  let mut out=[0;6];fs.read(b"/p/main.rs",0,&mut out).unwrap();
  assert_eq!(&out,b"source");
 }
 #[test]fn existing_destination_refuses_trash_restore_without_overwrite(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/trash/a",b"old").unwrap();
  fs.replace(b"/p/a",b"new").unwrap();
  assert_eq!(restore::<2,1024>(&mut fs,2,b"/trash/a",b"/p/a"),Err(Error::Conflict));
 }
}