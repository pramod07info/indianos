//! R024/10: bounded owner-generation workstation save queue with atomic RamFs publication.
//! Unsaved buffers remain queued when one destination fails. No disk durability implied.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Pending{path:[u8;PATH_MAX],len:usize,data:[u8;CONTENT_MAX],
 size:usize,expected:u64}
pub struct Saves<const N:usize>{owner:Identity,queue:[Option<Pending>;N],
 count:usize}
impl<const N:usize> Saves<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,queue:[None;N],count:0}}
 fn check(&self,caller:Identity)->Result<(),Error>{
  if caller.pid==0||caller!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn enqueue<const F:usize>(&mut self,caller:Identity,fs:&RamFs<F>,
  path:&[u8],data:&[u8])->Result<usize,Error>{
  self.check(caller)?;
  if path.len()<2||path.len()>PATH_MAX||data.len()>CONTENT_MAX{
   return Err(Error::Invalid);
  }
  if self.count>=N{return Err(Error::Full);}
  if self.queue.iter().flatten().any(|e|&e.path[..e.len]==path){
   return Err(Error::Conflict);
  }
  let (version,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut item=Pending{path:[0;PATH_MAX],len:path.len(),
   data:[0;CONTENT_MAX],size:data.len(),expected:version};
  item.path[..path.len()].copy_from_slice(path);
  item.data[..data.len()].copy_from_slice(data);
  self.queue[self.count]=Some(item);self.count+=1;Ok(self.count)
 }
 pub fn flush<const F:usize,const B:usize>(&mut self,caller:Identity,
  fs:&mut RamFs<F>)->Result<u64,Error>{
  self.check(caller)?;
  if self.count==0{return Err(Error::Missing);}
  for item in self.queue.iter().flatten(){
   let (version,_)=fs.metadata(&item.path[..item.len])
    .map_err(|_|Error::Missing)?;
   if version!=item.expected{return Err(Error::Stale);}
  }
  let mut bytes=[0u8;B];let n=fs.export_snapshot(&mut bytes)
   .map_err(|_|Error::Full)?;
  let mut staged=RamFs::<F>::new();
  staged.import_snapshot(&bytes[..n]).map_err(|_|Error::Invalid)?;
  for item in self.queue.iter().flatten(){
   staged.replace(&item.path[..item.len],&item.data[..item.size])
    .map_err(|_|Error::Full)?;
  }
  let mut final_data=[0u8;B];
  let n=staged.export_snapshot(&mut final_data).map_err(|_|Error::Full)?;
  fs.import_snapshot(&final_data[..n]).map_err(|_|Error::Invalid)?;
  self.queue=[None;N];self.count=0;Ok(fs.version())
 }
 pub fn discard(&mut self,caller:Identity)->Result<(),Error>{
  self.check(caller)?;self.queue=[None;N];self.count=0;Ok(())
 }
 pub fn pending(&self,caller:Identity)->Result<usize,Error>{
  self.check(caller)?;Ok(self.count)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn save_all_commits_two_open_real_source_documents(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"old a").unwrap();fs.replace(b"/b",b"old b").unwrap();
  let mut q=Saves::<2>::new(id(1));
  q.enqueue(id(1),&fs,b"/a",b"new a").unwrap();
  q.enqueue(id(1),&fs,b"/b",b"new b").unwrap();
  q.flush::<2,1024>(id(1),&mut fs).unwrap();
  let mut out=[0;5];fs.read(b"/b",0,&mut out).unwrap();
  assert_eq!(&out,b"new b");
 }
 #[test]fn stale_document_keeps_both_unsaved_files_queued(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  let mut q=Saves::<2>::new(id(1));
  q.enqueue(id(1),&fs,b"/a",b"x").unwrap();
  q.enqueue(id(1),&fs,b"/b",b"y").unwrap();
  fs.replace(b"/b",b"foreign").unwrap();
  assert_eq!(q.flush::<2,1024>(id(1),&mut fs),Err(Error::Stale));
  assert_eq!(q.pending(id(1)),Ok(2));
 }
 #[test]fn recycled_user_cannot_flush_old_unsaved_session(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut q=Saves::<1>::new(id(1));
  q.enqueue(id(1),&fs,b"/a",b"b").unwrap();
  assert_eq!(q.flush::<1,512>(id(2),&mut fs),Err(Error::Denied));
 }
}
