//! R022/10: staged project restore with two independent snapshot validations.
//! Actual RamFs import is atomic, but RAM is lost on reboot.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub struct Restore<const B:usize>{owner:Identity,
 buffer:[u8;B],size:usize,prepared:bool,original:u64}
impl<const B:usize> Restore<B>{
 pub const fn new(owner:Identity)->Self{
  Self{owner,buffer:[0;B],size:0,prepared:false,original:0}
 }
 pub fn prepare<const F:usize>(&mut self,actor:Identity,
  source:&RamFs<F>)->Result<usize,Error>{
  if actor.pid==0||actor.generation==0||actor!=self.owner{
   return Err(Error::Denied);
  }
  let mut image=[0u8;B];
  let size=source.export_snapshot(&mut image).map_err(|_|Error::Full)?;
  let mut verify=RamFs::<F>::new();
  verify.import_snapshot(&image[..size]).map_err(|_|Error::Invalid)?;
  self.buffer=image;self.size=size;self.prepared=true;
  self.original=source.version();Ok(size)
 }
 pub fn dry_run<const F:usize>(&self,actor:Identity)
  ->Result<(usize,u64),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}
  if !self.prepared{return Err(Error::Missing);}
  let mut result=RamFs::<F>::new();
  result.import_snapshot(&self.buffer[..self.size]).map_err(|_|Error::Invalid)?;
  Ok((result.count(),result.version()))
 }
 pub fn commit<const F:usize>(&mut self,actor:Identity,
  destination:&mut RamFs<F>,expected:u64)->Result<u64,Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}
  if !self.prepared{return Err(Error::Missing);}
  if destination.version()!=expected{return Err(Error::Stale);}
  let mut verified=RamFs::<F>::new();
  verified.import_snapshot(&self.buffer[..self.size]).map_err(|_|Error::Invalid)?;
  destination.import_snapshot(&self.buffer[..self.size])
   .map_err(|_|Error::Invalid)?;
  self.prepared=false;Ok(destination.version())
 }
 pub fn clear(&mut self,actor:Identity)->Result<(),Error>{
  if actor.pid==0||actor!=self.owner{return Err(Error::Denied);}
  self.buffer=[0;B];self.size=0;self.prepared=false;Ok(())
 }
 pub fn source_version(&self)->u64{self.original}
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn restores_real_exported_file_bytes_into_clean_workspace(){
  let mut source=RamFs::<1>::new();source.replace(b"/main",b"main").unwrap();
  let mut target=RamFs::<1>::new();
  let mut restore=Restore::<512>::new(id(1));
  restore.prepare(id(1),&source).unwrap();
  assert_eq!(restore.dry_run::<1>(id(1)),Ok((1,1)));
  restore.commit(id(1),&mut target,0).unwrap();
  let mut out=[0;4];target.read(b"/main",0,&mut out).unwrap();
  assert_eq!(&out,b"main");
 }
 #[test]fn recycled_identity_and_stale_target_block_restore(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  let mut s=Restore::<512>::new(id(1));s.prepare(id(1),&fs).unwrap();
  assert_eq!(s.dry_run::<1>(id(2)),Err(Error::Denied));
  fs.replace(b"/a",b"b").unwrap();
  assert_eq!(s.commit(id(1),&mut fs,1),Err(Error::Stale));
 }
 #[test]fn truncated_snapshot_buffer_does_not_mark_restore_prepared(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"hello").unwrap();
  let mut s=Restore::<4>::new(id(1));
  assert_eq!(s.prepare(id(1),&fs),Err(Error::Full));
  assert_eq!(s.dry_run::<1>(id(1)),Err(Error::Missing));
 }
}
