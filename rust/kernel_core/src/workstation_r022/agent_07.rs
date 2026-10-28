//! R022/07: revision-bound editor source bookmarks and navigation state.
//! Bookmark validation checks actual kernel file metadata before navigation.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Bookmark{pub owner:u32,pub id:u32,pub path:[u8;PATH_MAX],
 pub len:usize,pub version:u64,pub byte:usize,pub label:u32}
pub struct Bookmarks<const N:usize>{data:[Option<Bookmark>;N]}
impl<const N:usize> Bookmarks<N>{
 pub const fn new()->Self{Self{data:[None;N]}}
 fn index(&self,owner:u32,id:u32)->Option<usize>{
  self.data.iter().position(|x|x.is_some_and(|b|b.owner==owner&&b.id==id))
 }
 pub fn pin<const F:usize>(&mut self,owner:u32,id:u32,fs:&RamFs<F>,
  path:&[u8],offset:usize,label:u32)->Result<(),Error>{
  if owner==0||id==0||label==0||path.len()>PATH_MAX{
   return Err(Error::Invalid);
  }
  let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  if offset>size||size>CONTENT_MAX{return Err(Error::Invalid);}
  if self.index(owner,id).is_some(){return Err(Error::Conflict);}
  let free=self.data.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut bytes=[0u8;PATH_MAX];bytes[..path.len()].copy_from_slice(path);
  self.data[free]=Some(Bookmark{owner,id,path:bytes,len:path.len(),
   version,byte:offset,label});Ok(())
 }
 pub fn resolve<const F:usize>(&self,owner:u32,id:u32,fs:&RamFs<F>)
  ->Result<Bookmark,Error>{
  if owner==0{return Err(Error::Denied);}
  let b=self.index(owner,id).and_then(|i|self.data[i]).ok_or(Error::Missing)?;
  let (version,size)=fs.metadata(&b.path[..b.len]).map_err(|_|Error::Missing)?;
  if version!=b.version||b.byte>size{return Err(Error::Stale);}
  Ok(b)
 }
 pub fn retarget<const F:usize>(&mut self,owner:u32,id:u32,fs:&RamFs<F>,
  offset:usize)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  let mut b=self.data[i].ok_or(Error::Missing)?;
  let (version,size)=fs.metadata(&b.path[..b.len]).map_err(|_|Error::Missing)?;
  if offset>size{return Err(Error::Invalid);}
  b.version=version;b.byte=offset;self.data[i]=Some(b);Ok(())
 }
 pub fn remove(&mut self,owner:u32,id:u32)->Result<(),Error>{
  let i=self.index(owner,id).ok_or(Error::Missing)?;
  self.data[i]=None;Ok(())
 }
 pub fn list(&self,owner:u32,out:&mut[Bookmark])->Result<usize,Error>{
  if owner==0{return Err(Error::Denied);}
  let total=self.data.iter().flatten().filter(|x|x.owner==owner).count();
  if total>out.len(){return Err(Error::Full);}
  let mut count=0;for b in self.data.iter().flatten(){
   if b.owner==owner{out[count]=*b;count+=1;}
  }Ok(count)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn bookmark_jumps_to_real_file_offset(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/main",b"fn main").unwrap();
  let mut bookmarks=Bookmarks::<2>::new();
  bookmarks.pin(7,1,&fs,b"/src/main",3,99).unwrap();
  assert_eq!(bookmarks.resolve(7,1,&fs).unwrap().byte,3);
 }
 #[test]fn source_change_invalidates_old_revision_bookmark(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/main",b"old").unwrap();
  let mut b=Bookmarks::<1>::new();b.pin(7,1,&fs,b"/main",1,99).unwrap();
  fs.replace(b"/main",b"new").unwrap();
  assert_eq!(b.resolve(7,1,&fs),Err(Error::Stale));
  b.retarget(7,1,&fs,2).unwrap();
  assert_eq!(b.resolve(7,1,&fs).unwrap().byte,2);
 }
 #[test]fn foreign_bookmarks_are_not_enumerated(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abc").unwrap();
  let mut b=Bookmarks::<1>::new();b.pin(7,1,&fs,b"/a",1,99).unwrap();
  assert_eq!(b.resolve(8,1,&fs),Err(Error::Missing));
  let mut out: [Bookmark;0]=[];assert_eq!(b.list(7,&mut out),Err(Error::Full));
 }
}
