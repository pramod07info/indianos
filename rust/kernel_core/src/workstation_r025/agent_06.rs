//! R025/06 atomic project-level editor save-all with real RamFs snapshots.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Edit{path:[u8;PATH_MAX],len:usize,bytes:[u8;CONTENT_MAX],size:usize,revision:u64}
pub struct Saves<const N:usize>{owner:Identity,changes:[Option<Edit>;N],count:usize}
impl<const N:usize> Saves<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,changes:[None;N],count:0}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn stage<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
  path:&[u8],data:&[u8])->Result<(),Error>{
  self.auth(actor)?;
  if path.len()<2||path.len()>PATH_MAX||data.len()>CONTENT_MAX||self.count>=N{
   return Err(Error::Full);
  }
  if self.changes.iter().flatten().any(|x|&x.path[..x.len]==path){
   return Err(Error::Conflict);
  }
  let (revision,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut e=Edit{path:[0;PATH_MAX],len:path.len(),bytes:[0;CONTENT_MAX],
   size:data.len(),revision};
  e.path[..path.len()].copy_from_slice(path);
  e.bytes[..data.len()].copy_from_slice(data);
  self.changes[self.count]=Some(e);self.count+=1;Ok(())
 }
 pub fn save<const F:usize,const B:usize>(&mut self,actor:Identity,
  fs:&mut RamFs<F>,expected:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected!=fs.version(){return Err(Error::Stale);}
  if self.count==0{return Err(Error::Missing);}
  for e in self.changes.iter().flatten(){
   if fs.metadata(&e.path[..e.len]).map_err(|_|Error::Missing)?.0!=e.revision{
    return Err(Error::Stale);
   }
  }
  let mut image=[0u8;B];let n=fs.export_snapshot(&mut image).map_err(|_|Error::Full)?;
  let mut staged=RamFs::<F>::new();
  staged.import_snapshot(&image[..n]).map_err(|_|Error::Invalid)?;
  for e in self.changes.iter().flatten(){
   staged.replace(&e.path[..e.len],&e.bytes[..e.size]).map_err(|_|Error::Full)?;
  }
  let mut final_image=[0u8;B];let n=staged.export_snapshot(&mut final_image)
   .map_err(|_|Error::Full)?;
  fs.import_snapshot(&final_image[..n]).map_err(|_|Error::Invalid)?;
  self.changes=[None;N];self.count=0;Ok(fs.version())
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
 #[test]fn two_real_source_files_saved_in_one_snapshot(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"one").unwrap();fs.replace(b"/b",b"two").unwrap();
  let mut q=Saves::<2>::new(id(1));
  q.stage(id(1),&fs,b"/a",b"ONE").unwrap();
  q.stage(id(1),&fs,b"/b",b"TWO").unwrap();
  q.save::<2,2048>(id(1),&mut fs,2).unwrap();
  let mut out=[0u8;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"ONE");
 }
 #[test]fn stale_second_file_preserves_queued_changes(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  let mut q=Saves::<2>::new(id(1));
  q.stage(id(1),&fs,b"/a",b"A").unwrap();
  q.stage(id(1),&fs,b"/b",b"B").unwrap();
  fs.replace(b"/b",b"changed").unwrap();
  assert_eq!(q.save::<2,2048>(id(1),&mut fs,3),Err(Error::Stale));
  assert_eq!(q.pending(id(1)),Ok(2));
 }
 #[test]fn recycled_identity_cannot_flush_or_discard_session(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut q=Saves::<1>::new(id(1));q.stage(id(1),&fs,b"/a",b"A").unwrap();
  assert_eq!(q.save::<1,512>(id(2),&mut fs,1),Err(Error::Denied));
  assert_eq!(q.discard(id(2)),Err(Error::Denied));
 }
}
