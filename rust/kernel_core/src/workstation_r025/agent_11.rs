//! R025/11: run a bounded sequence of typed read-only commands atomically.
//! No pipes to host shell, no arbitrary scripts, and no partial output on error.
use super::{Error,agent_10};
use crate::batch7_ramfs::RamFs;
#[derive(Clone,Copy)]
pub struct Step<'a>{pub command:agent_10::Command,pub path:&'a[u8],
 pub param:&'a[u8]}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Record{pub step:usize,pub offset:usize,pub len:usize}
pub const EMPTY:Record=Record{step:0,offset:0,len:0};
pub fn execute<const F:usize,const B:usize,const N:usize>(
 fs:&RamFs<F>,steps:&[Step<'_>],output:&mut[u8;B],
 index:&mut[Record;N])->Result<usize,Error>{
 if steps.is_empty()||steps.len()>N{return Err(Error::Invalid);}
 let mut stage=[0u8;B];let mut records=[EMPTY;N];let mut cursor=0usize;
 for (i,s) in steps.iter().enumerate(){
  let mut temp=[0u8;2048];
  let n=agent_10::run(fs,s.path,s.command,s.param,&mut temp)?;
  if cursor.checked_add(n).is_none_or(|x|x>B){return Err(Error::Full);}
  stage[cursor..cursor+n].copy_from_slice(&temp[..n]);
  records[i]=Record{step:i,offset:cursor,len:n};cursor+=n;
 }
 *output=stage;*index=records;Ok(cursor)
}
pub fn step_output<'a>(output:&'a[u8],record:Record)->Option<&'a[u8]>{
 output.get(record.offset..record.offset.checked_add(record.len)?)
}
pub fn total_bytes(records:&[Record])->Result<usize,Error>{
 records.iter().try_fold(0usize,|n,r|n.checked_add(r.len).ok_or(Error::Overflow))
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch7_ramfs::RamFs;
 #[test]fn executes_cat_and_wordcount_on_same_real_file_as_single_result(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one\ntwo\n").unwrap();
  let steps=[Step{command:agent_10::Command::Cat,path:b"/a",param:b""},
   Step{command:agent_10::Command::WordCount,path:b"/a",param:b""}];
  let mut out=[0u8;30];let mut idx=[EMPTY;2];
  assert_eq!(execute(&fs,&steps,&mut out,&mut idx),Ok(9));
  assert_eq!(&out[..8],b"one\ntwo\n");
  assert_eq!(step_output(&out,idx[1]),Some(b"2".as_slice()));
 }
 #[test]fn missing_second_file_rolls_back_first_output(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let steps=[Step{command:agent_10::Command::Cat,path:b"/a",param:b""},
   Step{command:agent_10::Command::Cat,path:b"/missing",param:b""}];
  let mut out=[77;8];let mut idx=[EMPTY;2];
  assert_eq!(execute(&fs,&steps,&mut out,&mut idx),Err(Error::Missing));
  assert_eq!(out,[77;8]);
 }
 #[test]fn bounded_job_output_refuses_overflow_without_partial_results(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"long").unwrap();
  let steps=[Step{command:agent_10::Command::Cat,path:b"/a",param:b""}];
  let mut out=[77;2];let mut idx=[EMPTY;1];
  assert_eq!(execute(&fs,&steps,&mut out,&mut idx),Err(Error::Full));
  assert_eq!(idx,[EMPTY]);
 }
}
