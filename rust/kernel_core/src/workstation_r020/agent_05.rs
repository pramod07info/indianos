//! R020/05 atomic multi-file move/copy action under one source revision.
//! Stages actual RAM filesystem bytes first; no partial move on collision.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{Rename,Copy}
pub struct Change<'a>{pub from:&'a[u8],pub to:&'a[u8],pub mode:Action}
pub fn execute<const N:usize,const B:usize>(fs:&mut RamFs<N>,
 expected:u64,changes:&[Change<'_>])->Result<u64,Error>{
 if changes.is_empty()||changes.len()>N||fs.version()!=expected{
  return Err(Error::Stale);
 }
 for (i,action) in changes.iter().enumerate(){
  if action.from.is_empty()||action.to.is_empty()||
   action.from.len()>PATH_MAX||action.to.len()>PATH_MAX||
   action.from==action.to||changes[..i].iter().any(|other|
    other.to==action.to||other.from==action.from){
   return Err(Error::Conflict);
  }
 }
 let mut image=[0u8;B];
 let size=fs.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<N>::new();
 staged.import_snapshot(&image[..size]).map_err(|_|Error::Invalid)?;
 for change in changes{
  match change.mode{
   Action::Rename=>{
    let (ver,_) = staged.metadata(change.from).map_err(|_|Error::Missing)?;
    staged.rename_if_version(change.from,change.to,ver)
     .map_err(|_|Error::Conflict)?;
   }
   Action::Copy=>{staged.copy_new(change.from,change.to)
    .map_err(|_|Error::Conflict)?;}
  }
 }
 let mut final_image=[0u8;B];
 let len=staged.export_snapshot(&mut final_image).map_err(|_|Error::Full)?;
 if fs.version()!=expected{return Err(Error::Stale);}
 fs.import_snapshot(&final_image[..len]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preview<const N:usize>(fs:&RamFs<N>,actions:&[Change<'_>])
 ->Result<usize,Error>{
 if actions.is_empty()||actions.len()>N{return Err(Error::Invalid);}
 let mut count=0;
 for a in actions{
  fs.metadata(a.from).map_err(|_|Error::Missing)?;
  if fs.metadata(a.to).is_ok(){return Err(Error::Conflict);}
  count+=1;
 }Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn renames_and_copies_two_real_files_under_one_revision(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/src/a",b"a").unwrap();fs.replace(b"/src/b",b"b").unwrap();
  let actions=[Change{from:b"/src/a",to:b"/new/a",mode:Action::Rename},
   Change{from:b"/src/b",to:b"/new/b",mode:Action::Copy}];
  assert_eq!(preview(&fs,&actions),Ok(2));
  execute::<3,1024>(&mut fs,2,&actions).unwrap();
  assert!(fs.metadata(b"/new/a").is_ok());assert!(fs.metadata(b"/new/b").is_ok());
 }
 #[test]fn second_collision_rolls_back_first_rename(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  let actions=[Change{from:b"/a",to:b"/new",mode:Action::Rename},
   Change{from:b"/b",to:b"/new",mode:Action::Rename}];
  assert_eq!(execute::<2,1024>(&mut fs,2,&actions),Err(Error::Conflict));
  assert!(fs.metadata(b"/a").is_ok());
 }
 #[test]fn stale_version_never_changes_source_names(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let change=[Change{from:b"/a",to:b"/b",mode:Action::Rename}];
  assert_eq!(execute::<1,1024>(&mut fs,0,&change),Err(Error::Stale));
  assert!(fs.metadata(b"/b").is_err());
 }
}
