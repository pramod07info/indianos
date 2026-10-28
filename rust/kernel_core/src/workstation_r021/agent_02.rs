//! R021/02: owner/generation-bound atomic multi-document autosave to real RamFs.
//! No physical disk persistence is implied by a successful RAM transaction.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Change{path:[u8;PATH_MAX],path_len:usize,
 bytes:[u8;CONTENT_MAX],len:usize}
pub struct AutoSave<const N:usize>{
 owner:Identity,baseline:u64,changes:[Option<Change>;N],count:usize
}
impl<const N:usize> AutoSave<N>{
 pub const fn new(owner:Identity,baseline:u64)->Self{
  Self{owner,baseline,changes:[None;N],count:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor.generation==0||actor!=self.owner{
   return Err(Error::Denied);
  }Ok(())
 }
 pub fn stage(&mut self,actor:Identity,path:&[u8],data:&[u8])
  ->Result<usize,Error>{
  self.auth(actor)?;
  if path.len()<2||path.len()>PATH_MAX||path[0]!=b'/'||
   path.windows(2).any(|w|w==b"..")||data.len()>CONTENT_MAX{
   return Err(Error::Invalid);
  }
  if self.changes.iter().flatten().any(|c|&c.path[..c.path_len]==path){
   return Err(Error::Conflict);
  }
  if self.count>=N{return Err(Error::Full);}
  let mut item=Change{path:[0;PATH_MAX],path_len:path.len(),
   bytes:[0;CONTENT_MAX],len:data.len()};
  item.path[..path.len()].copy_from_slice(path);
  item.bytes[..data.len()].copy_from_slice(data);
  self.changes[self.count]=Some(item);self.count+=1;Ok(self.count)
 }
 pub fn commit<const F:usize,const B:usize>(&mut self,actor:Identity,
  fs:&mut RamFs<F>)->Result<u64,Error>{
  self.auth(actor)?;
  if self.count==0{return Err(Error::Missing);}
  if fs.version()!=self.baseline{return Err(Error::Stale);}
  let mut snapshot=[0u8;B];
  let n=fs.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
  let mut staged=RamFs::<F>::new();
  staged.import_snapshot(&snapshot[..n]).map_err(|_|Error::Invalid)?;
  for item in self.changes.iter().flatten(){
   staged.replace(&item.path[..item.path_len],&item.bytes[..item.len])
    .map_err(|_|Error::Full)?;
  }
  let len=staged.export_snapshot(&mut snapshot).map_err(|_|Error::Full)?;
  fs.import_snapshot(&snapshot[..len]).map_err(|_|Error::Invalid)?;
  self.baseline=fs.version();self.count=0;self.changes=[None;N];
  Ok(fs.version())
 }
 pub fn pending(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.count)
 }
 pub fn discard(&mut self,actor:Identity)->Result<(),Error>{
  self.auth(actor)?;self.changes=[None;N];self.count=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn batch_saves_two_real_workspace_files(){
  let mut fs=RamFs::<2>::new();
  let mut session=AutoSave::<2>::new(id(1),0);
  session.stage(id(1),b"/a",b"one").unwrap();
  session.stage(id(1),b"/b",b"two").unwrap();
  assert_eq!(session.commit::<2,1024>(id(1),&mut fs),Ok(2));
  let mut read=[0;3];fs.read(b"/b",0,&mut read).unwrap();
  assert_eq!(&read,b"two");assert_eq!(session.pending(id(1)),Ok(0));
 }
 #[test]fn oversized_two_file_commit_does_not_modify_first_file(){
  let mut fs=RamFs::<1>::new();
  let mut s=AutoSave::<2>::new(id(1),0);
  s.stage(id(1),b"/a",b"a").unwrap();s.stage(id(1),b"/b",b"b").unwrap();
  assert_eq!(s.commit::<1,1024>(id(1),&mut fs),Err(Error::Full));
  assert_eq!(fs.count(),0);assert_eq!(s.pending(id(1)),Ok(2));
 }
 #[test]fn recycled_pid_and_external_revision_refuse_autosave(){
  let mut fs=RamFs::<1>::new();let mut s=AutoSave::<1>::new(id(1),0);
  s.stage(id(1),b"/a",b"a").unwrap();fs.replace(b"/a",b"new").unwrap();
  assert_eq!(s.commit::<1,1024>(id(1),&mut fs),Err(Error::Stale));
  assert_eq!(s.commit::<1,1024>(id(2),&mut fs),Err(Error::Denied));
 }
}
