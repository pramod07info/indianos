//! R025/03: bounded, owner-generation-isolated MRU file navigation.
//! Recent entries are checked against real file revisions before opening.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Entry{pub path:[u8;PATH_MAX],pub len:usize,
 pub rev:u64,pub opened:u64}
pub const EMPTY:Entry=Entry{path:[0;PATH_MAX],len:0,rev:0,opened:0};
pub struct Recents<const N:usize>{owner:Identity,items:[Option<Entry>;N],tick:u64}
impl<const N:usize> Recents<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N],tick:0}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn touch<const F:usize>(&mut self,actor:Identity,
  fs:&RamFs<F>,path:&[u8])->Result<(),Error>{
  self.auth(actor)?;
  if path.is_empty()||path.len()>PATH_MAX||N==0{return Err(Error::Invalid);}
  let (rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut next=self.items;
  let index=next.iter().position(|e|e.is_some_and(|e|
    &e.path[..e.len]==path)).or_else(||next.iter().position(Option::is_none))
    .unwrap_or_else(||next.iter().enumerate().filter_map(|(i,e)|e.map(|x|(i,x.opened)))
     .min_by_key(|x|x.1).map(|x|x.0).unwrap_or(0));
  let tick=self.tick.checked_add(1).ok_or(Error::Overflow)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  next[index]=Some(Entry{path:name,len:path.len(),rev,opened:tick});
  self.items=next;self.tick=tick;Ok(())
 }
 pub fn list(&self,actor:Identity,out:&mut[Entry])->Result<usize,Error>{
  self.auth(actor)?;
  let count=self.items.iter().flatten().count();
  if count>out.len(){return Err(Error::Full);}
  for (i,e) in self.items.iter().flatten().enumerate(){out[i]=*e;}
  out[..count].sort_unstable_by(|a,b|b.opened.cmp(&a.opened));Ok(count)
 }
 pub fn open<const F:usize>(&self,actor:Identity,fs:&RamFs<F>,
 path:&[u8])->Result<Entry,Error>{
  self.auth(actor)?;
  let item=self.items.iter().flatten().find(|e|&e.path[..e.len]==path)
   .copied().ok_or(Error::Missing)?;
  if fs.metadata(path).map_err(|_|Error::Missing)?.0!=item.rev{
   return Err(Error::Stale);
  }Ok(item)
 }
 pub fn remove(&mut self,actor:Identity,path:&[u8])->Result<(),Error>{
  self.auth(actor)?;
  let slot=self.items.iter().position(|e|e.is_some_and(|e|
   &e.path[..e.len]==path)).ok_or(Error::Missing)?;
  self.items[slot]=None;Ok(())
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.items.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn recent_project_files_are_sorted_by_most_recent_visit(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a",b"a").unwrap();fs.replace(b"/b",b"b").unwrap();
  let mut r=Recents::<2>::new(id(1));
  r.touch(id(1),&fs,b"/a").unwrap();r.touch(id(1),&fs,b"/b").unwrap();
  let mut out=[EMPTY;2];assert_eq!(r.list(id(1),&mut out),Ok(2));
  assert_eq!(&out[0].path[..out[0].len],b"/b");
 }
 #[test]fn updated_file_requires_revisiting_to_refresh_stale_recent(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let mut r=Recents::<1>::new(id(1));r.touch(id(1),&fs,b"/a").unwrap();
  fs.replace(b"/a",b"two").unwrap();
  assert_eq!(r.open(id(1),&fs,b"/a"),Err(Error::Stale));
 }
 #[test]fn owner_generation_isolates_recent_file_list(){
  let fs=RamFs::<1>::new();let r=Recents::<1>::new(id(1));
  assert_eq!(r.count(id(2)),Err(Error::Denied));
  assert_eq!(r.list(id(2),&mut []),Err(Error::Denied));
 }
}
