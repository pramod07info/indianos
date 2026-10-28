//! R024/17: typed workstation launch/shortcut manifest and live target validation.
//! Opens real files and known workbench tools, never arbitrary executable byte streams.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum App{Editor,Files,Terminal,TestViewer,BuildViewer}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Shortcut{pub owner:Identity,pub id:u32,pub app:App,
 pub path:[u8;PATH_MAX],pub len:usize,pub rights:u32,pub file_rev:u64}
pub struct Launcher<const N:usize>{owner:Identity,shortcuts:[Option<Shortcut>;N]}
impl<const N:usize> Launcher<N>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,shortcuts:[None;N]}
 }
 fn auth(&self,owner:Identity)->Result<(),Error>{
  if owner.pid==0||owner!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn pin<const F:usize>(&mut self,owner:Identity,fs:&RamFs<F>,
  id:u32,app:App,path:&[u8],rights:u32)->Result<(),Error>{
  self.auth(owner)?;
  if id==0||rights==0||path.len()>PATH_MAX||
   self.shortcuts.iter().flatten().any(|s|s.id==id){return Err(Error::Invalid);}
  let (file_rev,_)=if matches!(app,App::Editor|App::Files){
   fs.metadata(path).map_err(|_|Error::Missing)?
  }else{(fs.version(),0)};
  let at=self.shortcuts.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut p=[0u8;PATH_MAX];p[..path.len()].copy_from_slice(path);
  self.shortcuts[at]=Some(Shortcut{owner,id,app,path:p,len:path.len(),
   rights,file_rev});Ok(())
 }
 pub fn launch<const F:usize>(&self,owner:Identity,fs:&RamFs<F>,
  id:u32,granted:u32)->Result<Shortcut,Error>{
  self.auth(owner)?;
  let shortcut=self.shortcuts.iter().flatten().find(|s|s.id==id)
   .copied().ok_or(Error::Missing)?;
  if shortcut.rights&granted!=shortcut.rights{return Err(Error::Denied);}
  if matches!(shortcut.app,App::Editor|App::Files){
   let (rev,_)=fs.metadata(&shortcut.path[..shortcut.len])
    .map_err(|_|Error::Missing)?;
   if rev!=shortcut.file_rev{return Err(Error::Stale);}
  }
  Ok(shortcut)
 }
 pub fn unpin(&mut self,owner:Identity,id:u32)->Result<(),Error>{
  self.auth(owner)?;
  let at=self.shortcuts.iter().position(|s|s.is_some_and(|s|s.id==id))
   .ok_or(Error::Missing)?;
  self.shortcuts[at]=None;Ok(())
 }
 pub fn list(&self,owner:Identity,out:&mut[Shortcut])->Result<usize,Error>{
  self.auth(owner)?;
  let count=self.shortcuts.iter().flatten().count();
  if out.len()<count{return Err(Error::Full);}
  for (i,item) in self.shortcuts.iter().flatten().enumerate(){out[i]=*item;}
  Ok(count)
 }
 pub fn count(&self,owner:Identity)->Result<usize,Error>{
  self.auth(owner)?;Ok(self.shortcuts.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn launcher_pins_real_editor_source_file_and_opens_with_rights(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main.rs",b"main").unwrap();
  let mut l=Launcher::<1>::new(id(1));
  l.pin(id(1),&fs,1,App::Editor,b"/main.rs",0b11).unwrap();
  assert_eq!(l.launch(id(1),&fs,1,3).unwrap().app,App::Editor);
 }
 #[test]fn stale_file_shortcut_must_refresh_after_source_change(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut l=Launcher::<1>::new(id(1));
  l.pin(id(1),&fs,1,App::Editor,b"/a",1).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(l.launch(id(1),&fs,1,1),Err(Error::Stale));
 }
 #[test]fn foreign_owner_cannot_run_or_list_other_users_shortcuts(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut l=Launcher::<1>::new(id(1));
  l.pin(id(1),&fs,1,App::Editor,b"/a",1).unwrap();
  assert_eq!(l.launch(id(2),&fs,1,1),Err(Error::Denied));
  assert_eq!(l.count(id(2)),Err(Error::Denied));
 }
}
