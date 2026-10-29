//! R026/09: live TODO/FIXME issue explorer over actual Rust project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Tag{Todo,Fixme}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Task{pub path:[u8;PATH_MAX],pub len:usize,
 pub line:usize,pub offset:usize,pub revision:u64,pub tag:Tag}
pub const EMPTY:Task=Task{path:[0;PATH_MAX],len:0,line:0,
 offset:0,revision:0,tag:Tag::Todo};
pub fn scan<const F:usize,const O:usize>(fs:&RamFs<F>,
 root:&[u8],out:&mut[Task;O])->Result<usize,Error>{
 if root.first()!=Some(&b'/'){return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||!path[..len].ends_with(b".rs"){continue;}
  let mut bytes=[0u8;CONTENT_MAX];
  fs.read(&path[..len],0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
  let mut start=0;let mut line=1;
  for fragment in bytes[..size].split_inclusive(|&b|b==b'\n'){
   if let Some(pos)=fragment.windows(2).position(|w|w==b"//"){
    let comment=&fragment[pos+2..];
    for (needle,tag) in [(b"TODO".as_slice(),Tag::Todo),
     (b"FIXME".as_slice(),Tag::Fixme)]{
     if let Some(offset)=comment.windows(needle.len()).position(|w|w==needle){
      if count==O{return Err(Error::Full);}
      stage[count]=Task{path,len,line,offset:start+pos+2+offset,revision,tag};
      count+=1;
     }
    }
   }
   start+=fragment.len();line+=1;
  }
 }
 stage[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len])
  .then(a.line.cmp(&b.line)));
 *out=stage;Ok(count)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,task:Task)->bool{
 fs.metadata(&task.path[..task.len]).is_ok_and(|(v,_)|v==task.revision)
}
pub fn count_tag(items:&[Task],tag:Tag)->usize{
 items.iter().filter(|x|x.tag==tag).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_issue_index_finds_real_todo_and_fixme_comments(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/main.rs",b"// TODO build\nfn x(){} // FIXME error\n").unwrap();
  let mut out=[EMPTY;3];assert_eq!(scan(&fs,b"/p",&mut out),Ok(2));
  assert_eq!(count_tag(&out[..2],Tag::Fixme),1);
 }
 #[test]fn edited_source_invalidates_old_todo_positions(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"// TODO fix").unwrap();
  let mut out=[EMPTY;1];scan(&fs,b"/p",&mut out).unwrap();
  fs.replace(b"/p/a.rs",b"fixed").unwrap();assert!(!valid(&fs,out[0]));
 }
 #[test]fn too_many_findings_do_not_modify_existing_results(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a.rs",b"// TODO x\n// TODO y").unwrap();
  let mut out=[Task{line:77,..EMPTY};1];
  assert_eq!(scan(&fs,b"/p",&mut out),Err(Error::Full));
  assert_eq!(out[0].line,77);
 }
}
