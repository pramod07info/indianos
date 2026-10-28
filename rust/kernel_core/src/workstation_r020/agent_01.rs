//! R020/01 bounded project checkpoints with authenticated owner epochs.
//! Snapshots are RAM-backed; restoring never implies durable disk recovery.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch7_ramfs::RamFs;
pub struct Checkpoint<const B:usize>{owner:Identity,bytes:[u8;B],
 length:usize,revision:u64,active:bool}
impl<const B:usize> Checkpoint<B>{
 pub const fn new()->Self{Self{owner:Identity{pid:0,generation:0},
  bytes:[0;B],length:0,revision:0,active:false}}
 pub fn save<const N:usize>(&mut self,owner:Identity,
  fs:&RamFs<N>)->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Denied);}
  let mut data=[0u8;B];
  let length=fs.export_snapshot(&mut data).map_err(|_|Error::Full)?;
  self.bytes=data;self.length=length;self.owner=owner;
  self.revision=fs.version();self.active=true;Ok(length)
 }
 pub fn restore<const N:usize>(&self,owner:Identity,
  fs:&mut RamFs<N>,expected_revision:u64)->Result<u64,Error>{
  if !self.active||owner!=self.owner{return Err(Error::Denied);}
  if fs.version()!=expected_revision{return Err(Error::Stale);}
  let mut staged=RamFs::<N>::new();
  staged.import_snapshot(&self.bytes[..self.length])
    .map_err(|_|Error::Invalid)?;
  // Import only after all parsing/checksum/capacity checks pass.
  fs.import_snapshot(&self.bytes[..self.length]).map_err(|_|Error::Invalid)?;
  Ok(fs.version())
 }
 pub fn snapshot_size(&self,owner:Identity)->Result<usize,Error>{
  if !self.active||self.owner!=owner{return Err(Error::Denied);}
  Ok(self.length)
 }
 pub fn source_revision(&self,owner:Identity)->Result<u64,Error>{
  if !self.active||self.owner!=owner{return Err(Error::Denied);}
  Ok(self.revision)
 }
 pub fn clear(&mut self,owner:Identity)->Result<(),Error>{
  if owner!=self.owner{return Err(Error::Denied);}
  *self=Self::new();Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 #[test]fn project_checkpoint_restores_real_file_bytes(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/src/a",b"hello").unwrap();
  let mut cp=Checkpoint::<512>::new();cp.save(id(1),&fs).unwrap();
  fs.replace(b"/src/a",b"world").unwrap();let rev=fs.version();
  assert_eq!(cp.restore(id(1),&mut fs,rev),Ok(1));
  let mut out=[0u8;5];fs.read(b"/src/a",0,&mut out).unwrap();assert_eq!(&out,b"hello");
 }
 #[test]fn recycled_pid_cannot_read_or_restore_project(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"safe").unwrap();
  let mut cp=Checkpoint::<256>::new();cp.save(id(1),&fs).unwrap();
  assert_eq!(cp.snapshot_size(id(2)),Err(Error::Denied));
  assert_eq!(cp.restore(id(2),&mut fs,1),Err(Error::Denied));
 }
 #[test]fn snapshot_buffer_capacity_and_stale_revision_are_fail_atomic(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"source").unwrap();
  let mut cp=Checkpoint::<4>::new();
  assert_eq!(cp.save(id(1),&fs),Err(Error::Full));
  assert_eq!(cp.snapshot_size(id(1)),Err(Error::Denied));
  let mut good=Checkpoint::<256>::new();good.save(id(1),&fs).unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(good.restore(id(1),&mut fs,1),Err(Error::Stale));
 }
}
