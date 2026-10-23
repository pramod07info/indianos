//! R030.1/30.2 opt-in QEMU-only persistent filesystem + allowlisted app catalog.
//! Kernel mount is re-created for each single-core syscall: no borrowed static disk,
//! no DMA and no mutable shared cache. Never run on a real physical device.
use crate::block::{BlockDevice,BlockWindow,SECTOR_BYTES};
use crate::durablefs::{DurableFs,FsError,MAX_FILE_BYTES,MIN_SECTORS};
use crate::ata_pio::AtaPio;
pub const START:u64=24;
pub const COUNT:u64=100;
pub const MAX_TRANSFER:usize=256;
pub const SIGNATURE_V1:&[u8]=b"INDAPP01\x01";
pub const SIGNATURE_V2:&[u8]=b"INDAPP01\x02";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error {Disk,Denied,Invalid,NotFound,Io}
impl From<FsError> for Error {
 fn from(e:FsError)->Self{
  if e==FsError::NotFound{Self::NotFound}else{Self::Io}
 }
}
fn reserved(path:&str)->bool{matches!(path,"/app"|"/previous")}
pub struct Service<D:BlockDevice>{fs:DurableFs<D>}
impl<D:BlockDevice> Service<D>{
 pub fn mount(disk:D)->Result<Self,Error>{
  Ok(Self{fs:DurableFs::mount(disk)?})
 }
 pub fn format_blank(disk:D)->Result<Self,Error>{
  Ok(Self{fs:DurableFs::format_blank(disk)?})
 }
 pub fn file(&mut self,op:u64,path:&str,bytes:&mut [u8],used:usize)->Result<usize,Error>{
  if reserved(path)||bytes.len()>MAX_TRANSFER||used>bytes.len(){return Err(Error::Denied);}
  match op{
   1=>{self.fs.create_path(path,&bytes[..used])?;Ok(0)},
   2=>{self.fs.read_path(path,bytes).map_err(Into::into)},
   3=>{self.fs.overwrite_path(path,&bytes[..used])?;Ok(0)},
   4=>{self.fs.delete_path(path)?;Ok(0)},
   _=>Err(Error::Invalid),
  }
 }
 fn current(&mut self,path:&str)->Result<Option<u8>,Error>{
  let mut value=[0u8;16];
  match self.fs.read_path(path,&mut value){
   Ok(9) if &value[..9]==SIGNATURE_V1=>Ok(Some(1)),
   Ok(9) if &value[..9]==SIGNATURE_V2=>Ok(Some(2)),
   Err(FsError::NotFound)=>Ok(None),
   _=>Err(Error::Invalid),
  }
 }
 pub fn status(&mut self)->Result<u64,Error>{
  Ok(self.current("/app")?.unwrap_or(0) as u64)
 }
 pub fn action(&mut self,action:u64)->Result<u64,Error>{
  match action{
   0=>self.status(),
   1=>{
    if self.current("/app")?.is_some(){return Err(Error::Denied);}
    self.fs.create_path("/app",SIGNATURE_V1)?;
    Ok(1)
   },
   2=>{
    if self.current("/app")?!=Some(1)||self.current("/previous")?.is_some(){
     return Err(Error::Denied);
    }
    self.fs.create_path("/previous",SIGNATURE_V1)?;
    self.fs.overwrite_path("/app",SIGNATURE_V2)?;
    Ok(2)
   },
   3=>{
    if self.current("/app")?!=Some(2)||
       self.current("/previous")?!=Some(1){return Err(Error::Denied);}
    self.fs.overwrite_path("/app",SIGNATURE_V1)?;
    self.fs.delete_path("/previous")?;
    Ok(1)
   },
   4=>{
    if self.current("/app")?.is_none(){return Err(Error::NotFound);}
    self.fs.delete_path("/app")?;
    if self.current("/previous")?.is_some(){self.fs.delete_path("/previous")?;}
    Ok(0)
   },
   _=>Err(Error::Invalid)
  }
 }
}
fn access<R>(f:impl FnOnce(&mut Service<BlockWindow<'_,AtaPio>>)->Result<R,Error>)
 ->Result<R,Error>{
 let mut disk=unsafe{AtaPio::probe()}.map_err(|_|Error::Disk)?;
 // This feature may ONLY run on a disposable 128-sector QEMU raw image.
 if disk.sector_count()!=128 || COUNT<MIN_SECTORS{return Err(Error::Denied);}
 let volume=BlockWindow::new(&mut disk,START,COUNT).map_err(|_|Error::Disk)?;
 let mut fs=Service::mount(volume)?;
 f(&mut fs)
}
pub fn bootstrap()->bool{
 let Ok(mut disk)=(unsafe{AtaPio::probe()})else{return false;};
 if disk.sector_count()!=128{return false;}
 let Ok(mut volume)=BlockWindow::new(&mut disk,START,COUNT)else{return false;};
 let mut first=[0u8;SECTOR_BYTES];
 let mut second=[0u8;SECTOR_BYTES];
 if volume.read_sector(0,&mut first).is_err()||
    volume.read_sector(1,&mut second).is_err(){return false;}
 let blank=first.iter().all(|&b|b==0)&&second.iter().all(|&b|b==0);
 let result=if blank{
  // DurableFs checks all 100 sectors are empty, not just metadata.
  Service::format_blank(volume)
 }else{Service::mount(volume)};
 result.is_ok()
}
pub fn operation(op:u64,path:&str,payload:&mut [u8],used:usize)->Result<usize,Error>{
 access(|fs|fs.file(op,path,payload,used))
}
pub fn package(action:u64)->Result<u64,Error>{
 access(|fs|fs.action(action))
}
pub fn installed()->bool{
 access(|fs|fs.status()).is_ok_and(|v|v==1||v==2)
}
#[cfg(test)]
mod tests{
 use super::*;
 use crate::block::BlockError;
 struct Disk {blocks:[[u8;SECTOR_BYTES];COUNT as usize]}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{COUNT}
  fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.blocks.get(lba as usize).ok_or(BlockError::OutOfRange)?;Ok(())
  }
  fn write_sector(&mut self,lba:u64,value:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.blocks.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*value;Ok(())
  }
 }
 #[test]fn persistent_files_and_app_lifecycle(){
  let disk=Disk{blocks:[[0;SECTOR_BYTES];COUNT as usize]};
  let mut svc=Service::format_blank(disk).unwrap();
  assert_eq!(svc.action(0),Ok(0));
  assert_eq!(svc.action(2),Err(Error::Denied));
  let mut note=[0u8;MAX_TRANSFER];note[..5].copy_from_slice(b"hello");
  assert_eq!(svc.file(1,"/note",&mut note,5),Ok(0));
  assert_eq!(svc.file(1,"/app",&mut note,5),Err(Error::Denied));
  assert_eq!(svc.action(1),Ok(1));
  assert_eq!(svc.action(1),Err(Error::Denied));
  let mut svc=Service::mount(svc.fs.into_device()).unwrap();
  assert_eq!(svc.status(),Ok(1));
  note.fill(0);
  assert_eq!(svc.file(2,"/note",&mut note,0),Ok(5));
  assert_eq!(&note[..5],b"hello");
  note[..4].copy_from_slice(b"test");
  assert_eq!(svc.file(3,"/note",&mut note,4),Ok(0));
  assert_eq!(svc.action(2),Ok(2));
  let mut svc=Service::mount(svc.fs.into_device()).unwrap();
  assert_eq!(svc.action(3),Ok(1));
  assert_eq!(svc.action(4),Ok(0));
  assert_eq!(svc.action(4),Err(Error::NotFound));
  assert_eq!(svc.file(4,"/note",&mut note,0),Ok(0));
  assert_eq!(svc.file(2,"/note",&mut note,0),Err(Error::NotFound));
  assert_eq!(svc.file(1,"/../../x",&mut note,1),Err(Error::Io));
 }
}
