//! R028/01: rename an entire real RamFs folder in one transaction.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
fn under(path:&[u8],root:&[u8])->bool{path.starts_with(root)&&path.get(root.len())==Some(&b'/')}
pub fn move_tree<const F:usize,const B:usize>(fs:&mut RamFs<F>,expected:u64,
 from:&[u8],to:&[u8])->Result<usize,Error>{
 if fs.version()!=expected{return Err(Error::Stale);}
 if from.len()<2||to.len()<2||from.len()>PATH_MAX||to.len()>PATH_MAX||
 from[0]!=b'/'||to[0]!=b'/'||from==to||under(to,from)||
 from.windows(2).any(|w|w==b"..")||to.windows(2).any(|w|w==b".."){
  return Err(Error::Invalid);
 }
 let mut old=[[0u8;PATH_MAX];F];let mut new=[[0u8;PATH_MAX];F];
 let mut oldlen=[0usize;F];let mut newlen=[0usize;F];let mut revisions=[0u64;F];
 let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let(len,_,rev)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !under(&path[..len],from){continue;}
  let remainder=&path[from.len()..len];
  let size=to.len()+remainder.len();
  if size>PATH_MAX{return Err(Error::Full);}
  old[n]=path;oldlen[n]=len;new[n][..to.len()].copy_from_slice(to);
  new[n][to.len()..size].copy_from_slice(remainder);newlen[n]=size;
  revisions[n]=rev;n+=1;
 }
 if n==0{return Err(Error::Missing);}
 for j in 0..n{
  if fs.metadata(&new[j][..newlen[j]]).is_ok(){return Err(Error::Conflict);}
 }
 let mut snapshot=[0u8;B];let sz=fs.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
 let mut staged=RamFs::<F>::new();
 staged.import_snapshot(&snapshot[..sz]).map_err(|_|Error::Invalid)?;
 for i in 0..n{
  staged.rename_if_version(&old[i][..oldlen[i]],&new[i][..newlen[i]],
   revisions[i]).map_err(|_|Error::Conflict)?;
 }
 let mut image=[0u8;B];let size=staged.export_snapshot(&mut image).map_err(|_|Error::Full)?;
 fs.import_snapshot(&image[..size]).map_err(|_|Error::Invalid)?;Ok(n)
}
pub fn members<const F:usize>(fs:&RamFs<F>,prefix:&[u8])->Result<usize,Error>{
 let mut n=0;for i in 0..fs.count(){
  let mut path=[0;PATH_MAX];let(len,_,_)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if under(&path[..len],prefix){n+=1;}
 }Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_folder_rename_moves_two_files_preserving_data(){
  let mut fs=RamFs::<3>::new();
  fs.replace(b"/old/a",b"a").unwrap();fs.replace(b"/old/b",b"b").unwrap();
  fs.replace(b"/else/c",b"c").unwrap();
  assert_eq!(move_tree::<3,4096>(&mut fs,3,b"/old",b"/new"),Ok(2));
  assert!(fs.metadata(b"/new/a").is_ok());
  assert!(fs.metadata(b"/old/a").is_err());assert_eq!(members(&fs,b"/new"),Ok(2));
 }
 #[test]fn collision_prevents_partial_move_and_path_escape(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/old/a",b"a").unwrap();fs.replace(b"/new/a",b"c").unwrap();
  assert_eq!(move_tree::<2,2048>(&mut fs,2,b"/old",b"/new"),Err(Error::Conflict));
  assert_eq!(move_tree::<2,2048>(&mut fs,2,b"/old",b"/old/sub"),Err(Error::Invalid));
 }
}