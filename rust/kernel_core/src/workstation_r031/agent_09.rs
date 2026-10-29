//! R031/09: live source TODO/FIXME navigation using real file revisions.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Task{pub path:[u8;PATH_MAX],pub path_len:usize,pub offset:usize,
 pub line:usize,pub fixme:bool,pub revision:u64}
pub const EMPTY:Task=Task{path:[0;PATH_MAX],path_len:0,offset:0,line:0,
 fixme:false,revision:0};
pub fn scan<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Task;N])->Result<usize,Error>{
 let mut stage=[EMPTY;N];let mut count=0;
 for i in 0..fs.count(){
  let mut name=[0;PATH_MAX];
  let (plen,len,revision)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !name[..plen].starts_with(root)||name.get(root.len())!=Some(&b'/')||
   !name[..plen].ends_with(b".rs"){continue;}
  let mut bytes=[0u8;CONTENT_MAX];fs.read(&name[..plen],0,&mut bytes[..len])
   .map_err(|_|Error::Missing)?;
  let mut line=1;
  for p in 0..len{
   let fixme=bytes[p..].starts_with(b"FIXME");
   let todo=bytes[p..].starts_with(b"TODO");
   if fixme||todo{
    if count==N{return Err(Error::Full);}
    stage[count]=Task{path:name,path_len:plen,offset:p,line,fixme,revision};
    count+=1;
   }
   if bytes[p]==b'\n'{line+=1;}
  }
 }
 *out=stage;Ok(count)
}
pub fn fresh<const F:usize>(fs:&RamFs<F>,task:Task)->bool{
 task.path_len<=PATH_MAX&&fs.metadata(&task.path[..task.path_len])
  .is_ok_and(|(rev,_)|rev==task.revision)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn indexes_real_task_markers_in_project_source(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/main.rs",b"// TODO\n// FIXME").unwrap();
  fs.replace(b"/q/private.rs",b"TODO").unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(scan(&fs,b"/p",&mut out),Ok(2));assert_eq!(out[1].line,2);
  assert!(out[1].fixme);
 }
 #[test]fn live_edit_invalidates_stale_task_navigation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"TODO").unwrap();
  let mut out=[EMPTY;1];scan(&fs,b"/p",&mut out).unwrap();
  fs.replace(b"/p/a.rs",b"done").unwrap();assert!(!fresh(&fs,out[0]));
 }
}