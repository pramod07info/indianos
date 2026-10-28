//! R022/14: editor working-session restore of tab/path/focus state.
//! Requires live file revision confirmation and exact PID/generation.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tab{pub path:[u8;PATH_MAX],pub len:usize,
 pub version:u64,pub cursor:usize,pub pinned:bool}
pub const EMPTY:Tab=Tab{path:[0;PATH_MAX],len:0,version:0,cursor:0,pinned:false};
pub struct Session<const N:usize>{owner:Identity,tabs:[Option<Tab>;N],
 active:usize,revision:u64}
impl<const N:usize> Session<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,tabs:[None;N],active:0,revision:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn open<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
  path:&[u8],cursor:usize)->Result<usize,Error>{
  self.auth(actor)?;
  if path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if cursor>size{return Err(Error::Invalid);}
  if self.tabs.iter().flatten().any(|t|&t.path[..t.len]==path){
   return Err(Error::Conflict);
  }
  let at=self.tabs.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  self.tabs[at]=Some(Tab{path:p,len:path.len(),version,cursor,pinned:false});
  self.active=at;self.revision=self.revision.checked_add(1)
   .ok_or(Error::Overflow)?;
  Ok(at)
 }
 pub fn focus(&mut self,actor:Identity,idx:usize,rev:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if rev!=self.revision{return Err(Error::Stale);}
  if self.tabs.get(idx).copied().flatten().is_none(){return Err(Error::Missing);}
  let next=rev.checked_add(1).ok_or(Error::Overflow)?;
  self.active=idx;self.revision=next;Ok(next)
 }
 pub fn validate<const F:usize>(&self,actor:Identity,fs:&RamFs<F>)
  ->Result<(),Error>{
  self.auth(actor)?;
  for t in self.tabs.iter().flatten(){
   let (version,size)=fs.metadata(&t.path[..t.len]).map_err(|_|Error::Missing)?;
   if t.version!=version||t.cursor>size{return Err(Error::Stale);}
  }Ok(())
 }
 pub fn close(&mut self,actor:Identity,idx:usize)->Result<(),Error>{
  self.auth(actor)?;
  let slot=self.tabs.get_mut(idx).ok_or(Error::Missing)?;
  if slot.is_none(){return Err(Error::Missing);}
  *slot=None;
  if self.active==idx{self.active=self.tabs.iter().position(Option::is_some).unwrap_or(0);}
  self.revision=self.revision.checked_add(1).ok_or(Error::Overflow)?;
  Ok(())
 }
 pub fn active(&self,actor:Identity)->Result<Option<Tab>,Error>{
  self.auth(actor)?;Ok(self.tabs.get(self.active).copied().flatten())
 }
 pub fn count(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.tabs.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn restores_real_open_tabs_and_focused_source(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/src/a",b"a").unwrap();
  fs.replace(b"/src/b",b"b").unwrap();
  let mut s=Session::<2>::new(id(1));
  s.open(id(1),&fs,b"/src/a",1).unwrap();
  s.open(id(1),&fs,b"/src/b",0).unwrap();
  assert_eq!(s.active(id(1)).unwrap().unwrap().cursor,0);
  assert_eq!(s.validate(id(1),&fs),Ok(()));
 }
 #[test]fn external_file_revision_invalidates_restored_editor_state(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut s=Session::<1>::new(id(1));s.open(id(1),&fs,b"/a",2).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(s.validate(id(1),&fs),Err(Error::Stale));
 }
 #[test]fn recycled_pid_is_denied_access_to_old_open_tabs(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"x").unwrap();
  let mut s=Session::<1>::new(id(1));s.open(id(1),&fs,b"/a",0).unwrap();
  assert_eq!(s.count(id(2)),Err(Error::Denied));
  assert_eq!(s.close(id(2),0),Err(Error::Denied));
 }
}
