//! R032/04: verify a captured project manifest before build/deploy.
use super::{Error,agent_02};
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Audit{pub missing:usize,pub stale:usize,pub added:usize}
pub fn verify<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 baseline:&[agent_02::Entry],out:&mut[usize;N])->Result<Audit,Error>{
 let mut dirty=[0usize;N];let mut count=0;
 let mut report=Audit{missing:0,stale:0,added:0};
 for(i,item) in baseline.iter().enumerate(){
  if item.len<2||item.len>PATH_MAX||
   !item.path[..item.len].starts_with(root){return Err(Error::Invalid);}
  if !agent_02::valid(fs,*item){
   if count==N{return Err(Error::Full);}
   dirty[count]=i;count+=1;
   if fs.metadata(&item.path[..item.len]).is_ok(){report.stale+=1;}
   else{report.missing+=1;}
  }
 }
 let mut actual=[agent_02::EMPTY;F];
 let n=agent_02::inspect(fs,root,&mut actual).map_err(|_|Error::Full)?;
 for file in &actual[..n]{
  if !baseline.iter().any(|e|e.len==file.len&&e.path[..e.len]==file.path[..file.len]){
   report.added+=1;
  }
 }
 out[..count].copy_from_slice(&dirty[..count]);Ok(report)
}
pub fn clean(report:Audit)->bool{report.missing==0&&report.stale==0&&report.added==0}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn captures_changed_and_new_real_files_after_baseline(){
  let mut fs=RamFs::<3>::new();fs.replace(b"/p/a",b"old").unwrap();
  let mut base=[agent_02::EMPTY;1];agent_02::inspect(&fs,b"/p",&mut base).unwrap();
  fs.replace(b"/p/a",b"new").unwrap();fs.replace(b"/p/b",b"added").unwrap();
  let mut ids=[0;1];let report=verify(&fs,b"/p",&base,&mut ids).unwrap();
  assert_eq!(report,Audit{missing:0,stale:1,added:1});assert!(!clean(report));
 }
 #[test]fn unchanged_files_verify_and_short_error_is_atomic(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"stable").unwrap();
  let mut base=[agent_02::EMPTY;1];agent_02::inspect(&fs,b"/p",&mut base).unwrap();
  let mut ids=[99;1];assert!(clean(verify(&fs,b"/p",&base,&mut ids).unwrap()));
  assert_eq!(ids,[99]);fs.replace(b"/p/a",b"dirty").unwrap();
  assert_eq!(verify(&fs,b"/p",&base,&mut[0usize;0]),Err(Error::Full));
 }
}