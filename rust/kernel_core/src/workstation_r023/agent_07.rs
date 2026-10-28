//! R023/07: per-identity workspace session state and revision-safe restoration.
//! Preserves active project, tabs, caret positions, layout and dirty indicators.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tab{pub path:[u8;PATH_MAX],pub len:usize,pub fs_version:u64,
 pub caret:usize,pub dirty:bool,pub pinned:bool}
pub const EMPTY:Tab=Tab{path:[0;PATH_MAX],len:0,fs_version:0,
 caret:0,dirty:false,pinned:false};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Layout{Editor,Split,Debug}
pub struct Workspace<const N:usize>{owner:Identity,tabs:[Option<Tab>;N],
 current:usize,layout:Layout,revision:u64}
impl<const N:usize> Workspace<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,tabs:[None;N],current:0,layout:Layout::Editor,revision:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn open<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>,
  path:&[u8],caret:usize)->Result<usize,Error>{
  self.auth(actor)?;
  if path.len()>PATH_MAX{return Err(Error::Invalid);}
  let (rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if caret>size{return Err(Error::Invalid);}
  if self.tabs.iter().flatten().any(|t|&t.path[..t.len]==path){
   return Err(Error::Conflict);
  }
  let at=self.tabs.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut p=[0;PATH_MAX];p[..path.len()].copy_from_slice(path);
  self.tabs[at]=Some(Tab{path:p,len:path.len(),fs_version:rev,
   caret,dirty:false,pinned:false});
  self.current=at;self.revision+=1;Ok(at)
 }
 pub fn mark_dirty(&mut self,actor:Identity,at:usize,
  flag:bool)->Result<(),Error>{
  self.auth(actor)?;
  let mut t=self.tabs.get(at).copied().flatten().ok_or(Error::Missing)?;
  t.dirty=flag;self.tabs[at]=Some(t);self.revision+=1;Ok(())
 }
 pub fn switch(&mut self,actor:Identity,at:usize,layout:Layout,
  expected:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected!=self.revision{return Err(Error::Stale);}
  if self.tabs.get(at).copied().flatten().is_none(){return Err(Error::Missing);}
  self.current=at;self.layout=layout;self.revision+=1;Ok(self.revision)
 }
 pub fn restore_check<const F:usize>(&self,actor:Identity,fs:&RamFs<F>)
  ->Result<usize,Error>{
  self.auth(actor)?;
  let mut count=0;
  for t in self.tabs.iter().flatten(){
   let (version,size)=fs.metadata(&t.path[..t.len]).map_err(|_|Error::Missing)?;
   if t.fs_version!=version||t.caret>size{return Err(Error::Stale);}
   count+=1;
  }Ok(count)
 }
 pub fn close(&mut self,actor:Identity,index:usize,force:bool)
  ->Result<(),Error>{
  self.auth(actor)?;
  let tab=self.tabs.get(index).copied().flatten().ok_or(Error::Missing)?;
  if tab.dirty&&!force{return Err(Error::Conflict);}
  self.tabs[index]=None;
  if index==self.current{
   self.current=self.tabs.iter().position(Option::is_some).unwrap_or(0);
  }
  self.revision+=1;Ok(())
 }
 pub fn focused(&self,actor:Identity)->Result<(Option<Tab>,Layout,u64),Error>{
  self.auth(actor)?;Ok((self.tabs[self.current],self.layout,self.revision))
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn real_two_tab_project_restores_split_layout_and_focus(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a",b"a").unwrap();fs.replace(b"/src/b",b"b").unwrap();
  let mut w=Workspace::<2>::new(id(1));
  w.open(id(1),&fs,b"/src/a",1).unwrap();w.open(id(1),&fs,b"/src/b",0).unwrap();
  w.switch(id(1),0,Layout::Split,2).unwrap();
  assert_eq!(w.restore_check(id(1),&fs),Ok(2));
  assert_eq!(w.focused(id(1)).unwrap().1,Layout::Split);
 }
 #[test]fn unsaved_source_edit_blocks_accidental_workspace_tab_close(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut w=Workspace::<1>::new(id(1));w.open(id(1),&fs,b"/a",0).unwrap();
  w.mark_dirty(id(1),0,true).unwrap();
  assert_eq!(w.close(id(1),0,false),Err(Error::Conflict));
  assert_eq!(w.close(id(1),0,true),Ok(()));
 }
 #[test]fn pid_reuse_and_external_file_change_invalidate_session_restore(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut w=Workspace::<1>::new(id(1));w.open(id(1),&fs,b"/a",0).unwrap();
  assert_eq!(w.restore_check(id(2),&fs),Err(Error::Denied));
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(w.restore_check(id(1),&fs),Err(Error::Stale));
 }
}
