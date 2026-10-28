//! R024/02: recoverable file Trash for real kernel RamFs.
//! RAM-only reclamation; owner-generation and stale revision guards prevent unsafe restores.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy)]
pub struct Item{path:[u8;PATH_MAX],len:usize,
 bytes:[u8;CONTENT_MAX],size:usize,file_revision:u64}
pub struct Bin<const N:usize>{owner:Identity,items:[Option<Item>;N]}
impl<const N:usize> Bin<N>{
 pub const fn new(owner:Identity)->Self{Self{owner,items:[None;N]}}
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 pub fn trash<const F:usize>(&mut self,actor:Identity,fs:&mut RamFs<F>,
  path:&[u8],expected_global:u64)->Result<usize,Error>{
  self.auth(actor)?;
  if expected_global!=fs.version(){return Err(Error::Stale);}
  if path.is_empty()||path.len()>PATH_MAX{return Err(Error::Invalid);}
  let slot=self.items.iter().position(Option::is_none).ok_or(Error::Full)?;
  let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
  let mut image=Item{path:[0;PATH_MAX],len:path.len(),
   bytes:[0;CONTENT_MAX],size,file_revision:revision};
  image.path[..path.len()].copy_from_slice(path);
  fs.read(path,0,&mut image.bytes[..size]).map_err(|_|Error::Missing)?;
  fs.delete_if_version(path,revision).map_err(|_|Error::Stale)?;
  self.items[slot]=Some(image);Ok(slot)
 }
 pub fn restore<const F:usize>(&mut self,actor:Identity,fs:&mut RamFs<F>,
  slot:usize,expected_global:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected_global!=fs.version(){return Err(Error::Stale);}
  let image=self.items.get(slot).copied().flatten().ok_or(Error::Missing)?;
  let path=&image.path[..image.len];
  if fs.metadata(path).is_ok(){return Err(Error::Conflict);}
  let rev=fs.replace(path,&image.bytes[..image.size])
   .map_err(|_|Error::Full)?;
  self.items[slot]=None;Ok(rev)
 }
 pub fn discard(&mut self,actor:Identity,slot:usize)->Result<(),Error>{
  self.auth(actor)?;
  let entry=self.items.get_mut(slot).ok_or(Error::Missing)?;
  if entry.is_none(){return Err(Error::Missing);}
  *entry=None;Ok(())
 }
 pub fn pending(&self,actor:Identity)->Result<usize,Error>{
  self.auth(actor)?;Ok(self.items.iter().flatten().count())
 }
 pub fn preview(&self,actor:Identity,slot:usize,
  out:&mut[u8])->Result<usize,Error>{
  self.auth(actor)?;
  let item=self.items.get(slot).copied().flatten().ok_or(Error::Missing)?;
  if out.len()<item.size{return Err(Error::Full);}
  out[..item.size].copy_from_slice(&item.bytes[..item.size]);Ok(item.size)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn deleted_real_file_can_be_restored_with_unchanged_bytes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/source",b"hello").unwrap();
  let mut bin=Bin::<1>::new(id(1));
  let slot=bin.trash(id(1),&mut fs,b"/source",1).unwrap();
  assert!(fs.metadata(b"/source").is_err());
  let rev=fs.version();bin.restore(id(1),&mut fs,slot,rev).unwrap();
  let mut out=[0;5];fs.read(b"/source",0,&mut out).unwrap();
  assert_eq!(&out,b"hello");assert_eq!(bin.pending(id(1)),Ok(0));
 }
 #[test]fn another_user_creating_same_file_blocks_restore_and_preserves_new_data(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"old").unwrap();
  let mut bin=Bin::<1>::new(id(1));let slot=bin.trash(id(1),&mut fs,b"/a",1).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(bin.restore(id(1),&mut fs,slot,fs.version()),Err(Error::Conflict));
  let mut out=[0;3];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"new");
 }
 #[test]fn recycled_process_identity_cannot_resurrect_other_users_trash(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut bin=Bin::<1>::new(id(1));let slot=bin.trash(id(1),&mut fs,b"/a",1).unwrap();
  assert_eq!(bin.restore(id(2),&mut fs,slot,fs.version()),Err(Error::Denied));
 }
}
