//! R031/04: integrity-checked project export and optimistic restore preview.
use super::Error;
use crate::batch7_ramfs::RamFs;
pub struct Snapshot<const B:usize>{bytes:[u8;B],size:usize,generation:u64}
impl<const B:usize> Snapshot<B>{
 pub fn capture<const F:usize>(fs:&RamFs<F>)->Result<Self,Error>{
  let mut data=[0u8;B];let size=fs.export_snapshot(&mut data).map_err(|_|Error::Full)?;
  Ok(Self{bytes:data,size,generation:fs.version()})
 }
 pub fn generation(&self)->u64{self.generation}
 pub fn preview<const F:usize>(&self,fs:&RamFs<F>)->Result<usize,Error>{
  let mut recovered=RamFs::<F>::new();
  recovered.import_snapshot(&self.bytes[..self.size]).map_err(|_|Error::Invalid)?;
  if fs.version()!=self.generation{return Err(Error::Stale);}
  Ok(recovered.count())
 }
 pub fn restore_if_generation<const F:usize>(&self,fs:&mut RamFs<F>,expected:u64)
  ->Result<usize,Error>{
  if fs.version()!=expected||expected!=self.generation{return Err(Error::Stale);}
  let count=self.preview(fs)?;
  fs.import_snapshot(&self.bytes[..self.size]).map_err(|_|Error::Invalid)?;
  Ok(count)
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn captures_and_restores_checked_guest_vfs_snapshot(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"data").unwrap();
  let snap=Snapshot::<2048>::capture(&fs).unwrap();
  assert_eq!(snap.preview(&fs),Ok(1));
  assert_eq!(snap.restore_if_generation(&mut fs,1),Ok(1));
  assert!(fs.metadata(b"/p/a").is_ok());
 }
 #[test]fn newer_file_generation_refuses_stale_snapshot_restore(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"old").unwrap();
  let snap=Snapshot::<1024>::capture(&fs).unwrap();
  fs.replace(b"/p/a",b"new").unwrap();
  assert_eq!(snap.restore_if_generation(&mut fs,2),Err(Error::Stale));
  let mut b=[0;3];fs.read(b"/p/a",0,&mut b).unwrap();assert_eq!(&b,b"new");
 }
}