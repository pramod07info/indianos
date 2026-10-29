//! R029/05: real guest workspace modification feed sorted by file generation.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Activity{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,pub bytes:usize}
pub const EMPTY:Activity=Activity{path:[0;PATH_MAX],len:0,revision:0,bytes:0};
pub fn recent<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Activity;N])->Result<usize,Error>{
 if root.first()!=Some(&b'/'){return Err(Error::Invalid);}
 let mut all=[EMPTY;F];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];let (n,bytes,revision)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..n].starts_with(root){continue;}
  all[count]=Activity{path,len:n,revision,bytes};count+=1;
 }
 all[..count].sort_unstable_by(|a,b|b.revision.cmp(&a.revision));
 let n=count.min(N);
 out[..n].copy_from_slice(&all[..n]);Ok(n)
}
pub fn newest<const F:usize>(fs:&RamFs<F>,root:&[u8])->Result<Option<Activity>,Error>{
 let mut out=[EMPTY;1];let n=recent(fs,root,&mut out)?;
 Ok(if n==0{None}else{Some(out[0])})
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn actual_file_generation_orders_recent_project_changes(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/p/a",b"a").unwrap();
  fs.replace(b"/p/b",b"b").unwrap();fs.replace(b"/p/a",b"new").unwrap();
  let mut out=[EMPTY;2];assert_eq!(recent(&fs,b"/p",&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/p/a");
 }
 #[test]fn zero_capacity_result_remains_valid_empty_feed(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut out=[EMPTY;0];assert_eq!(recent(&fs,b"/p",&mut out),Ok(0));
 }
}