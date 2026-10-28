//! R025/01: authenticated multi-project switcher over actual guest file tree.
//! Tracks independent project roots, validates current files and active focus.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Project{pub id:u32,pub root:[u8;PATH_MAX],pub len:usize,
 pub opened:u64,pub selected:bool}
pub struct Switcher<const N:usize>{owner:Identity,projects:[Option<Project>;N],
 active:u32,epoch:u64}
impl<const N:usize> Switcher<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,projects:[None;N],active:0,epoch:0}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn slot(&self,id:u32)->Option<usize>{self.projects.iter().position(|p|
  p.is_some_and(|p|p.id==id))}
 pub fn open<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
  id:u32,root:&[u8])->Result<(),Error>{
  self.auth(who)?;
  if id==0||root.len()<2||root.len()>PATH_MAX||root[0]!=b'/'||
   root.ends_with(b"/")||root.windows(2).any(|x|x==b"..")||
   self.slot(id).is_some(){return Err(Error::Invalid);}
  let valid=(0..fs.count()).any(|i|{
   let mut path=[0u8;PATH_MAX];
   fs.entry(i,&mut path).ok().flatten().is_some_and(|(n,_,_)|
    path[..n].starts_with(root)&&path.get(root.len())==Some(&b'/'))
  });
  if !valid{return Err(Error::Missing);}
  let at=self.projects.iter().position(Option::is_none).ok_or(Error::Full)?;
  self.epoch=self.epoch.checked_add(1).ok_or(Error::Overflow)?;
  let mut name=[0u8;PATH_MAX];name[..root.len()].copy_from_slice(root);
  self.projects[at]=Some(Project{id,root:name,len:root.len(),opened:self.epoch,selected:false});
  if self.active==0{self.switch(who,id)?;}Ok(())
 }
 pub fn switch(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let pos=self.slot(id).ok_or(Error::Missing)?;
  for p in &mut self.projects{if let Some(ref mut p)=p{p.selected=p.id==id;}}
  self.active=self.projects[pos].ok_or(Error::Missing)?.id;
  self.epoch=self.epoch.checked_add(1).ok_or(Error::Overflow)?;Ok(())
 }
 pub fn active(&self,who:Identity)->Result<Project,Error>{
  self.auth(who)?;self.slot(self.active).and_then(|i|self.projects[i])
   .ok_or(Error::Missing)
 }
 pub fn close(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let index=self.slot(id).ok_or(Error::Missing)?;
  self.projects[index]=None;if self.active==id{
   self.active=0;
   if let Some(next)=self.projects.iter().flatten().next().map(|p|p.id){
    self.switch(who,next)?;
   }
  }Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.projects.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn user_switches_two_actual_ram_projects(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/alpha/main.rs",b"a").unwrap();fs.replace(b"/beta/main.rs",b"b").unwrap();
  let mut s=Switcher::<2>::new(id(1));
  s.open(id(1),&fs,1,b"/alpha").unwrap();s.open(id(1),&fs,2,b"/beta").unwrap();
  s.switch(id(1),2).unwrap();assert_eq!(s.active(id(1)).unwrap().id,2);
  s.close(id(1),2).unwrap();assert_eq!(s.active(id(1)).unwrap().id,1);
 }
 #[test]fn opening_nonexistent_project_does_not_allocate_workspace(){
  let fs=RamFs::<1>::new();let mut s=Switcher::<1>::new(id(1));
  assert_eq!(s.open(id(1),&fs,1,b"/missing"),Err(Error::Missing));
  assert_eq!(s.count(id(1)),Ok(0));
 }
 #[test]fn pid_reuse_cannot_access_other_project_list(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/app/a",b"x").unwrap();
  let mut s=Switcher::<1>::new(id(1));s.open(id(1),&fs,1,b"/app").unwrap();
  assert_eq!(s.active(id(2)),Err(Error::Denied));
 }
}
