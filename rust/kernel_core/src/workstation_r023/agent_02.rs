//! R023/02: batch real-file create/edit/delete with optimistic preconditions.
//! All changes are staged into a validated image then published in one RAM commit.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Op{Create,Replace,Delete}
pub struct Change<'a>{pub op:Op,pub path:&'a[u8],
 pub old_revision:u64,pub bytes:&'a[u8]}
pub fn execute<const F:usize,const IMAGE:usize>(
 fs:&mut RamFs<F>,expected:u64,changes:&[Change<'_>])
 ->Result<u64,Error>{
 if expected!=fs.version(){return Err(Error::Stale);}
 if changes.is_empty()||changes.len()>F{return Err(Error::Invalid);}
 for (i,c) in changes.iter().enumerate(){
  if c.path.len()<2||c.path.len()>PATH_MAX||c.path[0]!=b'/'||
   changes[..i].iter().any(|p|p.path==c.path){return Err(Error::Conflict);}
 }
 let mut bytes=[0u8;IMAGE];
 let len=fs.export_snapshot(&mut bytes).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&bytes[..len]).map_err(|_|Error::Invalid)?;
 for change in changes{
  let metadata=staged.metadata(change.path).ok();
  match change.op{
   Op::Create=>{
    if metadata.is_some()||change.old_revision!=0{return Err(Error::Conflict);}
    staged.replace(change.path,change.bytes).map_err(|_|Error::Full)?;
   }
   Op::Replace=>{
    if metadata.is_none_or(|(v,_)|v!=change.old_revision)||change.old_revision==0{
     return Err(Error::Stale);
    }
    staged.replace(change.path,change.bytes).map_err(|_|Error::Full)?;
   }
   Op::Delete=>{
    if !change.bytes.is_empty(){return Err(Error::Invalid);}
    if metadata.is_none_or(|(v,_)|v!=change.old_revision)||change.old_revision==0{
     return Err(Error::Stale);
    }
    staged.delete_if_version(change.path,change.old_revision)
     .map_err(|_|Error::Conflict)?;
   }
  }
 }
 let mut final_image=[0u8;IMAGE];
 let used=staged.export_snapshot(&mut final_image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_image[..used]).map_err(|_|Error::Invalid)?;
 Ok(fs.version())
}
pub fn preview<const F:usize>(fs:&RamFs<F>,changes:&[Change<'_>])
 ->Result<(usize,usize,usize),Error>{
 let mut counts=(0,0,0);
 if changes.is_empty(){return Err(Error::Invalid);}
 for (i,c) in changes.iter().enumerate(){
  if changes[..i].iter().any(|p|p.path==c.path){return Err(Error::Conflict);}
  let metadata=fs.metadata(c.path).ok();
  match c.op{
   Op::Create=>{if metadata.is_some(){return Err(Error::Conflict);}counts.0+=1;}
   Op::Replace=>{if metadata.is_none_or(|(v,_)|v!=c.old_revision){return Err(Error::Stale);}counts.1+=1;}
   Op::Delete=>{if metadata.is_none_or(|(v,_)|v!=c.old_revision){return Err(Error::Stale);}counts.2+=1;}
  }
 }Ok(counts)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn creates_replaces_and_deletes_multiple_real_files_in_one_commit(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a",b"old").unwrap();fs.replace(b"/b",b"bye").unwrap();
  let changes=[
   Change{op:Op::Replace,path:b"/a",old_revision:1,bytes:b"new"},
   Change{op:Op::Delete,path:b"/b",old_revision:2,bytes:b""},
   Change{op:Op::Create,path:b"/c",old_revision:0,bytes:b"hello"}];
  assert_eq!(preview(&fs,&changes),Ok((1,1,1)));
  execute::<3,4096>(&mut fs,2,&changes).unwrap();
  let mut out=[0;3];fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out,b"new");assert!(fs.metadata(b"/b").is_err());
  assert!(fs.metadata(b"/c").is_ok());
 }
 #[test]fn late_stale_precondition_rolls_back_earlier_real_file_replacement(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"a").unwrap();
  fs.replace(b"/b",b"b").unwrap();
  let ops=[Change{op:Op::Replace,path:b"/a",old_revision:1,bytes:b"X"},
    Change{op:Op::Delete,path:b"/b",old_revision:1,bytes:b""}];
  assert_eq!(execute::<2,1024>(&mut fs,2,&ops),Err(Error::Stale));
  let mut out=[0];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(out,[b'a']);
 }
 #[test]fn duplicate_mutation_target_is_rejected_atomically(){
  let mut fs=RamFs::<2>::new();
  let changes=[Change{op:Op::Create,path:b"/a",old_revision:0,bytes:b"x"},
   Change{op:Op::Create,path:b"/a",old_revision:0,bytes:b"y"}];
  assert_eq!(execute::<2,1024>(&mut fs,0,&changes),Err(Error::Conflict));
  assert_eq!(fs.count(),0);
 }
}
