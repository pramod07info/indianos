//! R030/01: fail-atomic recursive project directory copy using real guest RAM files.
//! Real bytes and revision preflight; caller must separately authorize both project roots.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
pub fn copy_tree<const F:usize,const B:usize>(fs:&mut RamFs<F>,
 expected:u64,from:&[u8],to:&[u8])->Result<usize,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if from.len()<2||to.len()<2||from==to||from.len()>PATH_MAX||
 to.len()>PATH_MAX||from[0]!=b'/'||to[0]!=b'/'||
 to.starts_with(from)&&to.get(from.len())==Some(&b'/'){return Err(Error::Invalid);}
 let mut sources=[[0u8;PATH_MAX];F];
 let mut targets=[[0u8;PATH_MAX];F];
 let mut source_len=[0usize;F];let mut target_len=[0usize;F];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let(n,_,_)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..n].starts_with(from)||path.get(from.len())!=Some(&b'/'){continue;}
  let tail=&path[from.len()..n];let size=to.len()+tail.len();
  if size>PATH_MAX{return Err(Error::Full);}
  sources[count]=path;source_len[count]=n;
  targets[count][..to.len()].copy_from_slice(to);
  targets[count][to.len()..size].copy_from_slice(tail);target_len[count]=size;
  if fs.metadata(&targets[count][..size]).is_ok(){return Err(Error::Conflict);}
  count+=1;
 }
 if count==0{return Err(Error::Missing);}
 let mut raw=[0u8;B];let n=fs.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 let mut stage=RamFs::<F>::new();stage.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;
 for i in 0..count{
  stage.copy_new(&sources[i][..source_len[i]],&targets[i][..target_len[i]])
   .map_err(|_|Error::Full)?;
 }
 let n=stage.export_snapshot(&mut raw).map_err(|_|Error::Full)?;
 fs.import_snapshot(&raw[..n]).map_err(|_|Error::Invalid)?;Ok(count)
}
pub fn preview<const F:usize>(fs:&RamFs<F>,from:&[u8])->Result<usize,Error>{
 let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let(n,_,_)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if path[..n].starts_with(from)&&path.get(from.len())==Some(&b'/'){count+=1;}
 }Ok(count)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn copies_real_nested_source_tree_without_mutating_originals(){
  let mut fs=RamFs::<4>::new();fs.replace(b"/old/a.rs",b"hello").unwrap();
  fs.replace(b"/old/sub/b.rs",b"world").unwrap();
  assert_eq!(copy_tree::<4,4096>(&mut fs,2,b"/old",b"/copy"),Ok(2));
  let mut out=[0;5];fs.read(b"/copy/sub/b.rs",0,&mut out).unwrap();
  assert_eq!(&out,b"world");assert_eq!(preview(&fs,b"/old"),Ok(2));
 }
 #[test]fn copy_failure_when_no_slots_preserves_existing_tree(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/old/a",b"safe").unwrap();
  assert_eq!(copy_tree::<1,2048>(&mut fs,1,b"/old",b"/new"),Err(Error::Full));
  assert!(fs.metadata(b"/new/a").is_err());assert_eq!(fs.version(),1);
 }
}