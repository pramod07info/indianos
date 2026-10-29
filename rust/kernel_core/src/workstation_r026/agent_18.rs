//! R026/18: bounded multi-file editor tabs and split-pane state over actual files.
//! Owner-generation isolation and revision-bound selected tabs.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Pane{Primary,Secondary}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tab{pub id:u32,pub path:[u8;PATH_MAX],pub len:usize,
 pub revision:u64,pub pane:Pane,pub active:bool,pub dirty:bool}
pub struct Tabs<const N:usize>{owner:Identity,tabs:[Option<Tab>;N]}
impl<const N:usize> Tabs<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,tabs:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{
  self.tabs.iter().position(|t|t.is_some_and(|t|t.id==id))
 }
 pub fn open<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
 id:u32,path:&[u8],pane:Pane)->Result<(),Error>{
  self.auth(who)?;
  if id==0||path.is_empty()||path.len()>PATH_MAX||self.slot(id).is_some(){
   return Err(Error::Invalid);
  }
  let (rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let at=self.tabs.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.tabs[at]=Some(Tab{id,path:name,len:path.len(),revision:rev,
   pane,active:true,dirty:false});
  for t in &mut self.tabs{
   if let Some(ref mut t)=t{if t.pane==pane&&t.id!=id{t.active=false;}}
  }Ok(())
 }
 pub fn focus(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;
  let i=self.slot(id).ok_or(Error::Missing)?;
  let pane=self.tabs[i].ok_or(Error::Missing)?.pane;
  for t in &mut self.tabs{
   if let Some(ref mut t)=t{if t.pane==pane{t.active=t.id==id;}}
  }Ok(())
 }
 pub fn dirty(&mut self,who:Identity,id:u32,value:bool)->Result<(),Error>{
  self.auth(who)?;
  let i=self.slot(id).ok_or(Error::Missing)?;
  let mut t=self.tabs[i].ok_or(Error::Missing)?;
  t.dirty=value;self.tabs[i]=Some(t);Ok(())
 }
 pub fn close(&mut self,who:Identity,id:u32,force:bool)->Result<(),Error>{
  self.auth(who)?;
  let i=self.slot(id).ok_or(Error::Missing)?;
  if self.tabs[i].is_some_and(|t|t.dirty)&&!force{return Err(Error::Conflict);}
  self.tabs[i]=None;Ok(())
 }
 pub fn get<const F:usize>(&self,who:Identity,fs:&RamFs<F>,id:u32)
  ->Result<Tab,Error>{
  self.auth(who)?;
  let tab=self.slot(id).and_then(|i|self.tabs[i]).ok_or(Error::Missing)?;
  if fs.metadata(&tab.path[..tab.len]).map_err(|_|Error::Missing)?.0!=tab.revision{
   return Err(Error::Stale);
  }Ok(tab)
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.tabs.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn two_editor_panels_track_real_source_files_independently(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/a.rs",b"a").unwrap();fs.replace(b"/b.rs",b"b").unwrap();
  let mut tabs=Tabs::<2>::new(id(1));
  tabs.open(id(1),&fs,1,b"/a.rs",Pane::Primary).unwrap();
  tabs.open(id(1),&fs,2,b"/b.rs",Pane::Secondary).unwrap();
  assert_eq!(tabs.count(id(1)),Ok(2));
  assert!(tabs.get(id(1),&fs,1).unwrap().active);
 }
 #[test]fn unsaved_code_tab_cannot_close_without_force(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut t=Tabs::<1>::new(id(1));t.open(id(1),&fs,1,b"/a",Pane::Primary).unwrap();
  t.dirty(id(1),1,true).unwrap();
  assert_eq!(t.close(id(1),1,false),Err(Error::Conflict));
  t.close(id(1),1,true).unwrap();
 }
 #[test]fn modified_source_file_invalidates_prior_editor_tab_version(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut t=Tabs::<1>::new(id(1));t.open(id(1),&fs,1,b"/a",Pane::Primary).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(t.get(id(1),&fs,1),Err(Error::Stale));
 }
}
