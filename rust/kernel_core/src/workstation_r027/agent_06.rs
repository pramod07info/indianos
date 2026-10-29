//! R027/06: authenticated editor bookmarks validated against live source revisions.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Bookmark{pub id:u32,pub path:[u8;PATH_MAX],pub len:usize,
 pub revision:u64,pub line:u32,pub offset:usize}
pub struct Bookmarks<const N:usize>{owner:Identity,entries:[Option<Bookmark>;N]}
impl<const N:usize> Bookmarks<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,entries:[None;N]}}
 fn auth(&self,who:Identity)->Result<(),Error>{
  if who.pid==0||who!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn pos(&self,id:u32)->Option<usize>{
  self.entries.iter().position(|x|x.is_some_and(|x|x.id==id))
 }
 pub fn add<const F:usize>(&mut self,who:Identity,fs:&RamFs<F>,
  id:u32,path:&[u8],line:u32)->Result<(),Error>{
  self.auth(who)?;
  if id==0||line==0||path.len()<2||path.len()>PATH_MAX||
   self.pos(id).is_some(){return Err(Error::Invalid);}
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut text=[0u8;CONTENT_MAX];
  fs.read(path,0,&mut text[..size]).map_err(|_|Error::Missing)?;
  let mut current=1;let mut offset=0;
  while current<line&&offset<size{
   if text[offset]==b'\n'{current+=1;}offset+=1;
  }
  if current!=line||offset>=size{return Err(Error::Invalid);}
  let at=self.entries.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.entries[at]=Some(Bookmark{id,path:name,len:path.len(),
   revision,line,offset});Ok(())
 }
 pub fn jump<const F:usize>(&self,who:Identity,fs:&RamFs<F>,
  id:u32)->Result<usize,Error>{
  self.auth(who)?;
  let b=self.pos(id).and_then(|i|self.entries[i]).ok_or(Error::Missing)?;
  if fs.metadata(&b.path[..b.len]).map_err(|_|Error::Missing)?.0!=b.revision{
   return Err(Error::Stale);
  }Ok(b.offset)
 }
 pub fn remove(&mut self,who:Identity,id:u32)->Result<(),Error>{
  self.auth(who)?;let i=self.pos(id).ok_or(Error::Missing)?;
  self.entries[i]=None;Ok(())
 }
 pub fn count(&self,who:Identity)->Result<usize,Error>{
  self.auth(who)?;Ok(self.entries.iter().flatten().count())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn real_source_bookmark_resolves_second_line_byte(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"first\nsecond\n").unwrap();
  let mut b=Bookmarks::<1>::new(id(1));
  b.add(id(1),&fs,1,b"/a",2).unwrap();assert_eq!(b.jump(id(1),&fs,1),Ok(6));
 }
 #[test]fn editor_source_change_invalidates_old_bookmark(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let mut b=Bookmarks::<1>::new(id(1));b.add(id(1),&fs,1,b"/a",1).unwrap();
  fs.replace(b"/a",b"two").unwrap();assert_eq!(b.jump(id(1),&fs,1),Err(Error::Stale));
 }
 #[test]fn other_process_generation_cannot_read_bookmark(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  let mut b=Bookmarks::<1>::new(id(1));b.add(id(1),&fs,1,b"/a",1).unwrap();
  assert_eq!(b.jump(id(2),&fs,1),Err(Error::Denied));
 }
}
