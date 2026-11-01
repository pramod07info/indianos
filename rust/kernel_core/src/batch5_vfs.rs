//! Batch 5 tasks 5,7: PID/epoch-bound read-only VFS file descriptor adapter.
//! A guest VFS backend supplies trusted file bytes; syscall ingress must
//! authenticate identity and copy buffers using separate safe user-copy APIs.
use crate::vfs_path::normalize_absolute;
pub const PATH_CAPACITY:usize=128;
pub const READ_CAPACITY:usize=512;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FsError{Invalid,Path,Full,Denied,NotFound,Overflow,Io,BackendViolation}
#[derive(Clone,Copy)]
struct Descriptor{handle:u64,pid:u32,generation:u64,path:[u8;PATH_CAPACITY],len:usize}
pub trait GuestVfs{
 fn read_at(&mut self,path:&[u8],offset:u64,out:&mut[u8])->Result<usize,()>;
}
pub struct FileTable<const N:usize>{entries:[Option<Descriptor>;N],next:u64}
impl<const N:usize> FileTable<N>{
 pub const fn new()->Self{Self{entries:[None;N],next:1}}
 pub fn open(&mut self,pid:u32,generation:u64,path:&str)->Result<u64,FsError>{
  if pid==0||generation==0||self.next==u64::MAX{return Err(FsError::Invalid);}
  let mut normalized=[0u8;PATH_CAPACITY];
  let len=normalize_absolute(path,&mut normalized).map_err(|_|FsError::Path)?;
  if len==1{return Err(FsError::Path);}
  let free=self.entries.iter_mut().find(|s|s.is_none()).ok_or(FsError::Full)?;
  let id=self.next;
  *free=Some(Descriptor{handle:id,pid,generation,path:normalized,len});
  self.next+=1;Ok(id)
 }
 pub fn read<B:GuestVfs>(&self,pid:u32,generation:u64,id:u64,at:u64,
  destination:&mut[u8],backend:&mut B)->Result<usize,FsError>{
  if pid==0||generation==0||id==0||destination.len()>READ_CAPACITY{
   return Err(FsError::Invalid);
  }
  let file=self.entries.iter().flatten().find(|s|s.handle==id)
   .ok_or(FsError::NotFound)?;
  if file.pid!=pid||file.generation!=generation{return Err(FsError::Denied);}
  at.checked_add(destination.len() as u64).ok_or(FsError::Overflow)?;
  let mut scratch=[0u8;READ_CAPACITY];
  let n=backend.read_at(&file.path[..file.len],at,&mut scratch[..destination.len()])
   .map_err(|_|FsError::Io)?;
  if n>destination.len(){return Err(FsError::BackendViolation);}
  destination[..n].copy_from_slice(&scratch[..n]);Ok(n)
 }
 pub fn close(&mut self,pid:u32,generation:u64,id:u64)->Result<(),FsError>{
  let slot=self.entries.iter_mut().find(|s|s.is_some_and(|e|e.handle==id&&id!=0))
   .ok_or(FsError::NotFound)?;
  if !slot.is_some_and(|e|e.pid==pid&&e.generation==generation){
   return Err(FsError::Denied);
  }
  *slot=None;Ok(())
 }
 pub fn retire(&mut self,pid:u32,generation:u64){
  for slot in &mut self.entries{
   if slot.is_some_and(|e|e.pid==pid&&e.generation==generation){*slot=None;}
  }
 }
}
#[cfg(test)]mod tests{
 use super::*;
 struct Stub{fail:bool}
 impl GuestVfs for Stub{
  fn read_at(&mut self,path:&[u8],offset:u64,buf:&mut[u8])->Result<usize,()>{
   if self.fail||path!=b"/docs/note"||offset>3{return Err(());}
   let data=b"abc";let n=data.len().min(buf.len());
   buf[..n].copy_from_slice(&data[..n]);Ok(n)
  }
 }
 #[test]fn descriptor_authentication_and_no_partial_copy(){
  let mut fs=FileTable::<2>::new();
  let id=fs.open(7,3,"//docs//./note").unwrap();
  let mut out=[0xaau8;5];let mut backend=Stub{fail:true};
  assert_eq!(fs.read(7,3,id,0,&mut out,&mut backend),Err(FsError::Io));
  assert_eq!(out,[0xaa;5]);backend.fail=false;
  assert_eq!(fs.read(8,3,id,0,&mut out,&mut backend),Err(FsError::Denied));
  assert_eq!(fs.read(7,3,id,0,&mut out,&mut backend),Ok(3));
  assert_eq!(&out[..3],b"abc");
  fs.retire(7,3);
  assert_eq!(fs.read(7,3,id,0,&mut out,&mut backend),Err(FsError::NotFound));
 }
 #[test]fn traversal_rejected_before_descriptor_creation(){
  let mut fs=FileTable::<1>::new();
  assert_eq!(fs.open(1,1,"/a/../secret"),Err(FsError::Path));
  assert_eq!(fs.open(1,1,"/"),Err(FsError::Path));
  assert!(fs.open(1,1,"/ok").is_ok());
 }
}

/// R085 M1 integration: wire read-only descriptor use to the same
/// scheduler-derived PID-generation ACL checked by file service operations.
/// These entry points must be called only with the current PCB identity;
/// ordinary open/read remain trusted low-level helpers.
impl<const N:usize> FileTable<N>{
 pub fn open_authorized<const A:usize>(
  &mut self,rights:&crate::batch6_security::Access<A>,
  owner:crate::batch5_session::Identity,now:u64,path:&str)
  ->Result<u64,FsError>{
  rights.require(owner,crate::batch6_security::FILE_READ,now)
   .map_err(|_|FsError::Denied)?;
  self.open(owner.pid,owner.generation,path)
 }
 pub fn read_authorized<B:GuestVfs,const A:usize>(
  &self,rights:&crate::batch6_security::Access<A>,
  owner:crate::batch5_session::Identity,now:u64,id:u64,at:u64,
  output:&mut[u8],backend:&mut B)->Result<usize,FsError>{
  // Check current authority at EVERY read: a revoked/expired grant must
  // not keep working through a descriptor opened before revocation.
  rights.require(owner,crate::batch6_security::FILE_READ,now)
   .map_err(|_|FsError::Denied)?;
  self.read(owner.pid,owner.generation,id,at,output,backend)
 }
 pub fn close_owned(&mut self,
  owner:crate::batch5_session::Identity,id:u64)->Result<(),FsError>{
  self.close(owner.pid,owner.generation,id)
 }
}
#[cfg(test)]
mod r085_authorized_file_table_tests{
 use super::*;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
 struct Memory{bytes:[u8;8],len:usize}
 impl GuestVfs for Memory{
  fn read_at(&mut self,path:&[u8],at:u64,output:&mut[u8])
   ->Result<usize,()>{
   if path!=b"/user/note"||at>self.len as u64{return Err(());}
   let i=at as usize;
   let n=(self.len-i).min(output.len());
   output[..n].copy_from_slice(&self.bytes[i..i+n]);Ok(n)
  }
 }
 #[test]fn unauthorized_open_does_not_consume_handle_or_mutate_backend(){
  let reader=Identity{pid:4,generation:2};
  let mut access=Access::<2>::new();
  access.grant(reader,FILE_WRITE,20).unwrap();
  let mut files=FileTable::<1>::new();
  assert_eq!(files.open_authorized(&access,reader,1,"/user/note"),
   Err(FsError::Denied));
  access.revoke(reader).unwrap();
  access.grant(reader,FILE_READ,20).unwrap();
  let h=files.open_authorized(&access,reader,1,"/user/note").unwrap();
  assert_eq!(files.open_authorized(&access,reader,1,"/user/other"),
   Err(FsError::Full));
  let mut disk=Memory{bytes:*b"contents",len:8};
  let mut out=[0xaau8;8];
  assert_eq!(files.read_authorized(&access,
   Identity{generation:3,..reader},1,h,0,&mut out,&mut disk),
   Err(FsError::Denied));
  assert_eq!(out,[0xaa;8]);
  assert_eq!(files.read_authorized(&access,reader,20,h,0,&mut out,&mut disk),
   Err(FsError::Denied));
  assert_eq!(out,[0xaa;8]);
  assert_eq!(files.read_authorized(&access,reader,2,h,0,&mut out,&mut disk),
   Ok(8));
  assert_eq!(&out,b"contents");
  access.revoke(reader).unwrap();
  assert_eq!(files.read_authorized(&access,reader,2,h,0,&mut out,&mut disk),
   Err(FsError::Denied));
  assert_eq!(files.close_owned(Identity{generation:3,..reader},h),
   Err(FsError::Denied));
  files.close_owned(reader,h).unwrap();
  assert_eq!(files.open_authorized(&access,reader,2,"/user/note"),
   Err(FsError::Denied));
 }
 #[test]fn path_traversal_and_generation_lease_fail_without_publish(){
  let id=Identity{pid:8,generation:6};
  let mut acl=Access::<1>::new();acl.grant(id,FILE_READ,100).unwrap();
  let mut fd=FileTable::<2>::new();
  assert_eq!(fd.open_authorized(&acl,id,1,"/home/../system/secret"),
   Err(FsError::Path));
  let h=fd.open_authorized(&acl,id,1,"/user/note").unwrap();
  let mut disk=Memory{bytes:*b"contents",len:8};
  fd.retire(id.pid,id.generation);
  let mut output=[0u8;8];
  assert_eq!(fd.read_authorized(&acl,id,1,h,0,&mut output,&mut disk),
   Err(FsError::NotFound));
 }
}
