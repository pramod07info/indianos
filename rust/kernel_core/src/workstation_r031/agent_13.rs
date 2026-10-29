//! R031/13: authenticated terminal cwd navigation limited to actual guest project trees.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r030::agent_11 as resolver;
pub struct Cwd{owner:u64,root:[u8;PATH_MAX],root_len:usize,
 path:[u8;PATH_MAX],len:usize}
impl Cwd{
 pub fn new(owner:u64,root:&[u8])->Result<Self,Error>{
  if owner==0||root.len()<2||root.len()>PATH_MAX||root[0]!=b'/'||
   root.ends_with(b"/")||root.contains(&0){return Err(Error::Invalid);}
  let mut r=[0u8;PATH_MAX];r[..root.len()].copy_from_slice(root);
  Ok(Self{owner,root:r,root_len:root.len(),path:r,len:root.len()})
 }
 pub fn cd<const F:usize>(&mut self,owner:u64,fs:&RamFs<F>,relative:&[u8])
  ->Result<(),Error>{
  if owner!=self.owner||owner==0{return Err(Error::Denied);}
  let mut next=[0u8;PATH_MAX];
  let n=resolver::resolve(&self.root[..self.root_len],&self.path[..self.len],
    relative,&mut next).map_err(|_|Error::Denied)?;
  let mut exists=false;
  for i in 0..fs.count(){
   let mut entry=[0u8;PATH_MAX];
   let (m,_,_)=fs.entry(i,&mut entry).map_err(|_|Error::Invalid)?
    .ok_or(Error::Missing)?;
   if entry[..m].starts_with(&next[..n])&&entry.get(n)==Some(&b'/'){
    exists=true;break;
   }
  }
  if !exists{return Err(Error::Missing);}
  self.path=next;self.len=n;Ok(())
 }
 pub fn path<'a>(&'a self,owner:u64)->Result<&'a[u8],Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  Ok(&self.path[..self.len])
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn cd_enters_existing_directory_within_project_boundary(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/src/lib.rs",b"main").unwrap();
  let mut cwd=Cwd::new(7,b"/p").unwrap();cwd.cd(7,&fs,b"src").unwrap();
  assert_eq!(cwd.path(7),Ok(b"/p/src".as_slice()));
 }
 #[test]fn cross_project_or_missing_folder_cannot_change_current_directory(){
  let fs=RamFs::<0>::new();let mut cwd=Cwd::new(7,b"/p").unwrap();
  assert_eq!(cwd.cd(7,&fs,b"../q"),Err(Error::Denied));
  assert_eq!(cwd.cd(7,&fs,b"missing"),Err(Error::Missing));
  assert_eq!(cwd.path(7),Ok(b"/p".as_slice()));
 }
}