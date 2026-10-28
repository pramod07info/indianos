//! R022/02: atomic prefix-directory relocation on real RamFs workspace files.
//! Stages full export and validates every destination before changing live data.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
fn under(path:&[u8],prefix:&[u8])->bool{
 path.starts_with(prefix)&&(path.len()==prefix.len()||
  path.get(prefix.len())==Some(&b'/'))
}
pub fn relocate<const F:usize,const B:usize>(
 fs:&mut RamFs<F>,rev:u64,from:&[u8],to:&[u8])->Result<usize,Error>{
 if rev!=fs.version(){return Err(Error::Stale);}
 if from.len()<2||to.len()<2||from.len()>PATH_MAX||
  to.len()>PATH_MAX||from[0]!=b'/'||to[0]!=b'/'||
  from==to||under(to,from)||under(from,to)||
  from.windows(2).any(|v|v==b"..")||to.windows(2).any(|v|v==b".."){
  return Err(Error::Invalid);
 }
 let mut bytes=[0u8;B];
 let n=fs.export_snapshot(&mut bytes).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&bytes[..n]).map_err(|_|Error::Invalid)?;
 let mut moves=0;
 // Collect names from unchanged *original* to avoid index/order aliasing.
 for index in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,_,_)=fs.entry(index,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !under(&path[..len],from){continue;}
  let suffix=&path[from.len()..len];
  let destlen=to.len().checked_add(suffix.len()).ok_or(Error::Overflow)?;
  if destlen>PATH_MAX{return Err(Error::Full);}
  let mut newname=[0u8;PATH_MAX];
  newname[..to.len()].copy_from_slice(to);
  newname[to.len()..destlen].copy_from_slice(suffix);
  let (ver,_)=staged.metadata(&path[..len]).map_err(|_|Error::Missing)?;
  staged.rename_if_version(&path[..len],&newname[..destlen],ver)
   .map_err(|_|Error::Conflict)?;
  moves+=1;
 }
 if moves==0{return Err(Error::Missing);}
 let mut final_bytes=[0u8;B];
 let n=staged.export_snapshot(&mut final_bytes).map_err(|_|Error::Full)?;
 fs.import_snapshot(&final_bytes[..n]).map_err(|_|Error::Invalid)?;
 Ok(moves)
}
pub fn affected<const F:usize>(fs:&RamFs<F>,from:&[u8])->usize{
 (0..fs.count()).filter(|i|{
  let mut path=[0u8;PATH_MAX];
  fs.entry(*i,&mut path).ok().flatten()
   .is_some_and(|(len,_,_)|under(&path[..len],from))
 }).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn moves_entire_directory_with_real_file_contents(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/src/main.rs",b"main").unwrap();
  fs.replace(b"/src/lib.rs",b"lib").unwrap();
  fs.replace(b"/docs/a",b"docs").unwrap();
  assert_eq!(affected(&fs,b"/src"),2);
  assert_eq!(relocate::<3,2048>(&mut fs,3,b"/src",b"/code"),Ok(2));
  let mut out=[0;4];fs.read(b"/code/main.rs",0,&mut out).unwrap();
  assert_eq!(&out,b"main");assert!(fs.metadata(b"/docs/a").is_ok());
 }
 #[test]fn destination_collision_rolls_back_every_rename(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"a").unwrap();fs.replace(b"/dst/a",b"b").unwrap();
  assert_eq!(relocate::<2,1024>(&mut fs,2,b"/src",b"/dst"),Err(Error::Conflict));
  assert!(fs.metadata(b"/src/a").is_ok());assert_eq!(fs.version(),2);
 }
 #[test]fn unrelated_or_stale_prefix_is_rejected_without_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/a",b"a").unwrap();
  assert_eq!(relocate::<1,1024>(&mut fs,0,b"/src",b"/code"),Err(Error::Stale));
  assert_eq!(relocate::<1,1024>(&mut fs,1,b"/src",b"/src/sub"),Err(Error::Invalid));
 }
}
