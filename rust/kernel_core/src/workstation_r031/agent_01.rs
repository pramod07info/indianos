//! R031/01: atomically copy a whole live project directory with no destination overwrites.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
fn child(path:&[u8],dir:&[u8])->bool{path.starts_with(dir)&&path.get(dir.len())==Some(&b'/')}
pub fn clone_tree<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 from:&[u8],to:&[u8])->Result<usize,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if from.len()<2||to.len()<2||from.len()>PATH_MAX||to.len()>PATH_MAX||
  from[0]!=b'/'||to[0]!=b'/'||from==to||child(from,to)||child(to,from)||
  from.ends_with(b"/")||to.ends_with(b"/"){return Err(Error::Invalid);}
 let mut old=[[0u8;PATH_MAX];F];let mut new=[[0u8;PATH_MAX];F];
 let mut ol=[0usize;F];let mut nl=[0usize;F];let mut total=0;
 for i in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (n,_,_)=fs.entry(i,&mut name).map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !child(&name[..n],from){continue;}
  let dest=to.len().checked_add(n-from.len()).ok_or(Error::Overflow)?;
  if dest>PATH_MAX{return Err(Error::Full);}
  old[total]=name;ol[total]=n;new[total][..to.len()].copy_from_slice(to);
  new[total][to.len()..dest].copy_from_slice(&name[from.len()..n]);
  nl[total]=dest;total+=1;
 }
 if total==0{return Err(Error::Missing);}
 if fs.count()+total>F{return Err(Error::Full);}
 for i in 0..total{
  if fs.metadata(&new[i][..nl[i]]).is_ok(){return Err(Error::Conflict);}
 }
 let mut snapshot=[0u8;B];let n=fs.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&snapshot[..n]).map_err(|_|Error::Invalid)?;
 for i in 0..total{
  staged.copy_new(&old[i][..ol[i]],&new[i][..nl[i]])
    .map_err(|_|Error::Full)?;
 }
 let mut updated=[0u8;B];let n=staged.export_snapshot(&mut updated).map_err(|_|Error::Full)?;
 fs.import_snapshot(&updated[..n]).map_err(|_|Error::Invalid)?;Ok(total)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn copies_all_files_without_modifying_source(){
  let mut fs=RamFs::<5>::new();
  fs.replace(b"/p/src/a",b"foo").unwrap();fs.replace(b"/p/readme",b"hello").unwrap();
  assert_eq!(clone_tree::<5,8192>(&mut fs,2,b"/p",b"/backup"),Ok(2));
  assert_eq!(fs.count(),4);assert!(fs.metadata(b"/backup/src/a").is_ok());
  let mut result=[0;3];fs.read(b"/p/src/a",0,&mut result).unwrap();assert_eq!(&result,b"foo");
 }
 #[test]fn collision_or_full_capacity_preserves_original_generation(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"A").unwrap();
  fs.replace(b"/backup/a",b"B").unwrap();
  assert_eq!(clone_tree::<2,4096>(&mut fs,2,b"/p",b"/backup"),Err(Error::Full));
  assert_eq!(fs.version(),2);
 }
}