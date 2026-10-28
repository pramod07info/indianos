//! R023/06: grouped project-wide undo/redo via validated RAM file snapshots.
//! Real filesystem bytes are restored as one atomic snapshot, not individual UI flags.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
#[derive(Clone,Copy)]
pub struct Image<const B:usize>{pub data:[u8;B],pub len:usize,pub rev:u64}
pub struct History<const B:usize,const H:usize>{
 owner:Identity,undo:[Option<Image<B>>;H],undo_len:usize,
 redo:[Option<Image<B>>;H],redo_len:usize
}
impl<const B:usize,const H:usize> History<B,H>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,undo:[None;H],undo_len:0,redo:[None;H],redo_len:0}
 }
 fn auth(&self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}Ok(())
 }
 fn push(stack:&mut[Option<Image<B>>;H],len:&mut usize,item:Image<B>){
  if H==0{return;}
  if *len==H{for i in 1..H{stack[i-1]=stack[i];}*len-=1;}
  stack[*len]=Some(item);*len+=1;
 }
 fn capture<const F:usize>(fs:&RamFs<F>)->Result<Image<B>,Error>{
  let mut data=[0u8;B];let len=fs.export_snapshot(&mut data)
   .map_err(|_|Error::Full)?;
  Ok(Image{data,len,rev:fs.version()})
 }
 fn apply<const F:usize>(fs:&mut RamFs<F>,image:Image<B>)
  ->Result<u64,Error>{
  let mut staged=RamFs::<F>::new();
  staged.import_snapshot(&image.data[..image.len]).map_err(|_|Error::Invalid)?;
  fs.import_snapshot(&image.data[..image.len]).map_err(|_|Error::Invalid)?;
  Ok(fs.version())
 }
 pub fn checkpoint<const F:usize>(&mut self,actor:Identity,fs:&RamFs<F>)
  ->Result<(),Error>{
  self.auth(actor)?;
  if H==0{return Err(Error::Full);}
  let image=Self::capture(fs)?;
  Self::push(&mut self.undo,&mut self.undo_len,image);
  self.redo=[None;H];self.redo_len=0;Ok(())
 }
 pub fn undo<const F:usize>(&mut self,actor:Identity,fs:&mut RamFs<F>,
  expected:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected!=fs.version(){return Err(Error::Stale);}
  if self.undo_len==0{return Err(Error::Missing);}
  let current=Self::capture(fs)?;
  let previous=self.undo[self.undo_len-1].ok_or(Error::Missing)?;
  // Import can fail only if image no longer validates; never drop history first.
  let revision=Self::apply(fs,previous)?;
  Self::push(&mut self.redo,&mut self.redo_len,current);
  self.undo_len-=1;self.undo[self.undo_len]=None;Ok(revision)
 }
 pub fn redo<const F:usize>(&mut self,actor:Identity,fs:&mut RamFs<F>,
  expected:u64)->Result<u64,Error>{
  self.auth(actor)?;
  if expected!=fs.version(){return Err(Error::Stale);}
  if self.redo_len==0{return Err(Error::Missing);}
  let current=Self::capture(fs)?;
  let next=self.redo[self.redo_len-1].ok_or(Error::Missing)?;
  let revision=Self::apply(fs,next)?;
  Self::push(&mut self.undo,&mut self.undo_len,current);
  self.redo_len-=1;self.redo[self.redo_len]=None;Ok(revision)
 }
 pub fn depths(&self,actor:Identity)->Result<(usize,usize),Error>{
  self.auth(actor)?;Ok((self.undo_len,self.redo_len))
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  self.auth(actor)?;self.undo=[None;H];self.redo=[None;H];
  self.undo_len=0;self.redo_len=0;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn multi_document_history_restores_and_redoes_real_project_bytes(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"old").unwrap();
  let mut h=History::<1024,3>::new(id(1));
  h.checkpoint(id(1),&fs).unwrap();
  fs.replace(b"/a",b"new").unwrap();fs.replace(b"/b",b"new").unwrap();
  h.undo(id(1),&mut fs,3).unwrap();assert_eq!(fs.count(),1);
  h.redo(id(1),&mut fs,1).unwrap();assert_eq!(fs.count(),2);
 }
 #[test]fn stale_generation_and_external_revision_block_undo(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut h=History::<512,1>::new(id(1));h.checkpoint(id(1),&fs).unwrap();
  fs.replace(b"/a",b"b").unwrap();
  assert_eq!(h.undo(id(2),&mut fs,2),Err(Error::Denied));
  assert_eq!(h.undo(id(1),&mut fs,1),Err(Error::Stale));
 }
 #[test]fn oversized_snapshot_cannot_destroy_previous_history(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"1234567").unwrap();
  let mut h=History::<4,1>::new(id(1));
  assert_eq!(h.checkpoint(id(1),&fs),Err(Error::Full));
  assert_eq!(h.depths(id(1)),Ok((0,0)));
 }
}
