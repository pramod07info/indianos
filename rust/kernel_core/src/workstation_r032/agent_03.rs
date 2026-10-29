//! R032/03: compare two real project trees, pairing files by relative path.
use super::{Error,agent_02};
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Difference{Added,Removed,Modified}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Change{pub relative:[u8;PATH_MAX],pub len:usize,pub kind:Difference}
pub const EMPTY:Change=Change{relative:[0;PATH_MAX],len:0,kind:Difference::Modified};
pub fn diff<const F:usize,const N:usize>(fs:&RamFs<F>,old:&[u8],new:&[u8],
 out:&mut[Change;N])->Result<usize,Error>{
 if old==new||old.len()<2||new.len()<2{return Err(Error::Invalid);}
 let mut a=[agent_02::EMPTY;F];let mut b=[agent_02::EMPTY;F];
 let an=agent_02::inspect(fs,old,&mut a).map_err(|_|Error::Invalid)?;
 let bn=agent_02::inspect(fs,new,&mut b).map_err(|_|Error::Invalid)?;
 let mut staged=[EMPTY;N];let mut count=0;
 for item in &a[..an]{
  let rel=&item.path[old.len()..item.len];
  let right=b[..bn].iter().find(|e|&e.path[new.len()..e.len]==rel);
  let change=match right{
   None=>Some(Difference::Removed),
   Some(other)if other.checksum!=item.checksum||other.bytes!=item.bytes=>Some(Difference::Modified),
   _=>None,
  };
  if let Some(kind)=change{
   if count==N{return Err(Error::Full);}
   let mut relative=[0u8;PATH_MAX];relative[..rel.len()].copy_from_slice(rel);
   staged[count]=Change{relative,len:rel.len(),kind};count+=1;
  }
 }
 for item in &b[..bn]{
  let rel=&item.path[new.len()..item.len];
  if a[..an].iter().any(|e|&e.path[old.len()..e.len]==rel){continue;}
  if count==N{return Err(Error::Full);}
  let mut relative=[0u8;PATH_MAX];relative[..rel.len()].copy_from_slice(rel);
  staged[count]=Change{relative,len:rel.len(),kind:Difference::Added};count+=1;
 }
 staged[..count].sort_unstable_by(|x,y|x.relative[..x.len].cmp(&y.relative[..y.len]));
 *out=staged;Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn compares_modified_and_new_real_project_files(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/a/x.rs",b"old").unwrap();
  fs.replace(b"/b/x.rs",b"new").unwrap();fs.replace(b"/b/y.rs",b"added").unwrap();
  let mut changes=[EMPTY;2];assert_eq!(diff(&fs,b"/a",b"/b",&mut changes),Ok(2));
  assert_eq!(changes[0].kind,Difference::Modified);
  assert_eq!(changes[1].kind,Difference::Added);
 }
 #[test]fn unchanged_files_yield_empty_diff_and_capacity_errors_are_atomic(){
  let mut fs=RamFs::<4>::new();
  fs.replace(b"/a/a",b"same").unwrap();fs.replace(b"/b/a",b"same").unwrap();
  let mut changes=[EMPTY;0];assert_eq!(diff(&fs,b"/a",b"/b",&mut changes),Ok(0));
  fs.replace(b"/b/b",b"new").unwrap();
  assert_eq!(diff(&fs,b"/a",b"/b",&mut changes),Err(Error::Full));
 }
}