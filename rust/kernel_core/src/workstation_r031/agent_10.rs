//! R031/10: owner-scoped source bookmarks invalidated by file revision changes.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Bookmark{pub path:[u8;PATH_MAX],pub len:usize,pub rev:u64,pub byte:usize}
pub struct Bookmarks<const N:usize>{owner:u64,items:[Option<Bookmark>;N]}
impl<const N:usize> Bookmarks<N>{
 pub fn new(owner:u64)->Self{Self{owner,items:[None;N]}}
 pub fn pin<const F:usize>(&mut self,owner:u64,fs:&RamFs<F>,
  path:&[u8],byte:usize)->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let (rev,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if len>CONTENT_MAX||byte>len||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let mut buffer=[0u8;CONTENT_MAX];fs.read(path,0,&mut buffer[..len])
   .map_err(|_|Error::Missing)?;
  if !core::str::from_utf8(&buffer[..len]).map_err(|_|Error::Invalid)?
    .is_char_boundary(byte){return Err(Error::Invalid);}
  let i=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
  self.items[i]=Some(Bookmark{path:name,len:path.len(),rev,byte});Ok(i)
 }
 pub fn navigate<const F:usize>(&self,owner:u64,fs:&RamFs<F>,
  slot:usize)->Result<usize,Error>{
  if owner==0||owner!=self.owner{return Err(Error::Denied);}
  let b=self.items.get(slot).and_then(|b|*b).ok_or(Error::Missing)?;
  if !fs.metadata(&b.path[..b.len]).is_ok_and(|(v,_)|v==b.rev){
   return Err(Error::Stale);
  }
  Ok(b.byte)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn bookmark_points_into_actual_source_utf8_position(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a\nb").unwrap();
  let mut bookmarks=Bookmarks::<1>::new(7);
  let slot=bookmarks.pin(7,&fs,b"/p/a",2).unwrap();
  assert_eq!(bookmarks.navigate(7,&fs,slot),Ok(2));
 }
 #[test]fn bookmark_rejects_other_user_and_changed_file_generation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();
  let mut marks=Bookmarks::<1>::new(7);
  assert_eq!(marks.pin(8,&fs,b"/p/a",1),Err(Error::Denied));
  marks.pin(7,&fs,b"/p/a",1).unwrap();fs.replace(b"/p/a",b"b").unwrap();
  assert_eq!(marks.navigate(7,&fs,0),Err(Error::Stale));
 }
}