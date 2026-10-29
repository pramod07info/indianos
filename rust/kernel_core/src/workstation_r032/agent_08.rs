//! R032/08: ownership-isolated tabbed editor with unsaved-close protection.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tab{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,
 pub dirty:bool,pub selected:bool}
pub struct Session<const N:usize>{owner:u64,tabs:[Option<Tab>;N]}
impl<const N:usize> Session<N>{
 pub fn new(owner:u64)->Self{Self{owner,tabs:[None;N]}}
 fn check(&self,owner:u64)->Result<(),Error>{
  if owner==0||self.owner!=owner{Err(Error::Denied)}else{Ok(())}
 }
 pub fn open<const F:usize>(&mut self,owner:u64,fs:&RamFs<F>,
 path:&[u8])->Result<usize,Error>{
  self.check(owner)?;
  if path.len()>PATH_MAX{return Err(Error::Invalid);}
  let(rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if let Some(i)=self.tabs.iter().position(|t|t.is_some_and(|v|v.len==path.len()
     &&&v.path[..v.len]==path)){return Ok(i);}
  let i=self.tabs.iter().position(Option::is_none).ok_or(Error::Full)?;
  for tab in self.tabs.iter_mut().flatten(){tab.selected=false;}
  let mut buf=[0u8;PATH_MAX];buf[..path.len()].copy_from_slice(path);
  self.tabs[i]=Some(Tab{path:buf,len:path.len(),revision:rev,dirty:false,selected:true});Ok(i)
 }
 pub fn mark_dirty(&mut self,owner:u64,slot:usize)->Result<(),Error>{
  self.check(owner)?;let tab=self.tabs.get_mut(slot).and_then(Option::as_mut)
   .ok_or(Error::Missing)?;tab.dirty=true;Ok(())
 }
 pub fn save_verified<const F:usize>(&mut self,owner:u64,slot:usize,
  fs:&RamFs<F>)->Result<(),Error>{
  self.check(owner)?;
  let tab=self.tabs.get_mut(slot).and_then(Option::as_mut).ok_or(Error::Missing)?;
  let(rev,_)=fs.metadata(&tab.path[..tab.len]).map_err(|_|Error::Missing)?;
  if rev==tab.revision{return Err(Error::Stale);}
  tab.revision=rev;tab.dirty=false;Ok(())
 }
 pub fn close(&mut self,owner:u64,slot:usize,discard:bool)->Result<(),Error>{
  self.check(owner)?;
  let tab=self.tabs.get(slot).and_then(|x|*x).ok_or(Error::Missing)?;
  if tab.dirty&&!discard{return Err(Error::Conflict);}
  self.tabs[slot]=None;Ok(())
 }
 pub fn count(&self,owner:u64)->Result<usize,Error>{
  self.check(owner)?;Ok(self.tabs.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn open_existing_file_mark_dirty_and_save_after_real_write(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"old").unwrap();
  let mut s=Session::<2>::new(7);
  let id=s.open(7,&fs,b"/p/a").unwrap();s.mark_dirty(7,id).unwrap();
  assert_eq!(s.close(7,id,false),Err(Error::Conflict));
  fs.replace(b"/p/a",b"new").unwrap();s.save_verified(7,id,&fs).unwrap();
  s.close(7,id,false).unwrap();assert_eq!(s.count(7),Ok(0));
 }
 #[test]fn owner_separation_and_no_overbooking(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();
  fs.replace(b"/p/b",b"b").unwrap();let mut s=Session::<1>::new(7);
  assert_eq!(s.open(8,&fs,b"/p/a"),Err(Error::Denied));
  s.open(7,&fs,b"/p/a").unwrap();
  assert_eq!(s.open(7,&fs,b"/p/b"),Err(Error::Full));
 }
}