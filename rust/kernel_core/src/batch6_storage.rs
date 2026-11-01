//! R040 tasks 5/6/8/24: transactional guest-file update contract.
//! A backend is responsible for true disk flush/order, journaling, lock and
//! persistent power-loss safety; host unit tests cannot establish durability.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_WRITE};
use crate::vfs_path::normalize_absolute;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Unauthorized,TooLarge,Storage,Corrupt}
pub const PATH_CAP:usize=96;
pub const MAX_WRITE:usize=2048;
pub trait AtomicFileBackend {
 fn write_atomic(&mut self,path:&[u8],contents:&[u8])->Result<(),()>;
 fn read_all(&mut self,path:&[u8],output:&mut[u8])->Result<usize,()>;
}
pub fn replace_verified<B:AtomicFileBackend,const N:usize>(
 auth:&Access<N>,owner:Identity,now:u64,path:&str,
 image:&[u8],backend:&mut B)->Result<(),Error>{
 auth.require(owner,FILE_WRITE,now).map_err(|_|Error::Unauthorized)?;
 if image.is_empty()||image.len()>MAX_WRITE{return Err(Error::TooLarge);}
 let mut canonical=[0u8;PATH_CAP];
 let n=normalize_absolute(path,&mut canonical).map_err(|_|Error::Invalid)?;
 if n<2{return Err(Error::Invalid);}
 // R085: this File/Terminal write path must enforce the same protected
 // namespaces as the FileHandles ABI. Unsupported canonical paths fail closed.
 let canonical_path=core::str::from_utf8(&canonical[..n])
     .map_err(|_|Error::Invalid)?;
 crate::fs_write_guard::WriteGuard::allow_path(canonical_path)
     .map_err(|_|Error::Unauthorized)?;
 // The backend must flush a complete, checksummed COW generation before
 // publication. No partial update is accepted as a valid result.
 backend.write_atomic(&canonical[..n],image).map_err(|_|Error::Storage)?;
 Ok(())
}
pub fn read_verified<B:AtomicFileBackend>(backend:&mut B,path:&str,
 dst:&mut[u8])->Result<usize,Error>{
 if dst.len()>MAX_WRITE{return Err(Error::TooLarge);}
 let mut canonical=[0u8;PATH_CAP];
 let n=normalize_absolute(path,&mut canonical).map_err(|_|Error::Invalid)?;
 if n<2{return Err(Error::Invalid);}
 let mut scratch=[0u8;MAX_WRITE];
 let bytes=backend.read_all(&canonical[..n],&mut scratch[..dst.len()])
   .map_err(|_|Error::Storage)?;
 if bytes>dst.len(){return Err(Error::Corrupt);}
 dst[..bytes].copy_from_slice(&scratch[..bytes]);Ok(bytes)
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch6_security::Access;
 struct Disk{fail:bool,slot:[u8;16],len:usize}
 impl AtomicFileBackend for Disk{
  fn write_atomic(&mut self,path:&[u8],body:&[u8])->Result<(),()>{
   if self.fail||path!=b"/profile"||body.len()>16{return Err(());}
   self.slot=[0;16];self.slot[..body.len()].copy_from_slice(body);
   self.len=body.len();Ok(())
  }
  fn read_all(&mut self,path:&[u8],dest:&mut[u8])->Result<usize,()>{
   if self.fail||path!=b"/profile"||dest.len()<self.len{return Err(());}
   dest[..self.len].copy_from_slice(&self.slot[..self.len]);Ok(self.len)
  }
 }
 #[test]fn write_requires_rights_and_fails_without_mutation(){
  let a=Identity{pid:2,generation:4};
  let mut acl=Access::<1>::new();acl.grant(a,FILE_WRITE,100).unwrap();
  let mut disk=Disk{fail:true,slot:[0;16],len:0};
  assert_eq!(replace_verified(&acl,a,4,"//profile",b"hello",&mut disk),
   Err(Error::Storage));
  assert_eq!(disk.len,0);disk.fail=false;
  replace_verified(&acl,a,4,"/profile",b"hello",&mut disk).unwrap();
  let mut out=[0xaau8;8];
  assert_eq!(read_verified(&mut disk,"/profile",&mut out),Ok(5));
  assert_eq!(&out[..5],b"hello");
  assert_eq!(replace_verified(&acl,Identity{generation:5,..a},4,
   "/profile",b"bad",&mut disk),Err(Error::Unauthorized));
  assert_eq!(disk.len,5);
 }
}


/// R085: security-gated read wrapper for syscall-facing services. Use this
/// instead of trusted-only read_verified when the request comes from ring 3.
pub fn read_authorized<B:AtomicFileBackend,const N:usize>(
 auth:&Access<N>,owner:Identity,now:u64,backend:&mut B,
 path:&str,out:&mut[u8])->Result<usize,Error>{
 auth.require(owner,crate::batch6_security::FILE_READ,now)
    .map_err(|_|Error::Unauthorized)?;
 read_verified(backend,path,out)
}

/// Optional readback integrity check for synchronous, readable backends.
/// A mismatch is reported but is NOT a durable rollback guarantee; real
/// storage implementations must provide their own atomic commit/flush.
pub fn replace_and_verify<B:AtomicFileBackend,const N:usize>(
 auth:&Access<N>,owner:Identity,now:u64,path:&str,
 image:&[u8],backend:&mut B)->Result<(),Error>{
 replace_verified(auth,owner,now,path,image,backend)?;
 let mut scratch=[0u8;MAX_WRITE];
 let read=read_verified(backend,path,&mut scratch[..image.len()])?;
 if read!=image.len()||&scratch[..read]!=image{
    return Err(Error::Corrupt);
 }
 Ok(())
}
#[cfg(test)]
mod r085_storage_acceptance{
 use super::*;
 use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
 struct Memory{bytes:[u8;16],len:usize,corrupt:bool}
 impl AtomicFileBackend for Memory{
  fn write_atomic(&mut self,_path:&[u8],src:&[u8])->Result<(),()>{
    if src.len()>16{return Err(());}
    self.bytes[..src.len()].copy_from_slice(src);self.len=src.len();
    Ok(())
  }
  fn read_all(&mut self,_path:&[u8],out:&mut[u8])->Result<usize,()>{
    if out.len()<self.len{return Err(());}
    out[..self.len].copy_from_slice(&self.bytes[..self.len]);
    if self.corrupt&&self.len>0{out[0]^=1;}
    Ok(self.len)
  }
 }
 #[test]fn protected_namespace_denied_before_backend_write(){
  let owner=Identity{pid:3,generation:9};
  let mut acl=Access::<1>::new();acl.grant(owner,FILE_WRITE|FILE_READ,20).unwrap();
  let mut backend=Memory{bytes:[0;16],len:0,corrupt:false};
  for path in ["/boot/init","//system/./driver","/recovery/image",
               "/security/config","/snapshots/bak"]{
    assert_eq!(replace_verified(&acl,owner,1,path,b"unsafe",&mut backend),
        Err(Error::Unauthorized));
    assert_eq!(backend.len,0);
  }
  replace_verified(&acl,owner,1,"/workspace/file",b"safe",&mut backend).unwrap();
  assert_eq!(&backend.bytes[..backend.len],b"safe");
 }
 #[test]fn authenticated_reads_and_readback_reject_stale_or_corrupt(){
  let owner=Identity{pid:3,generation:9};
  let mut acl=Access::<1>::new();acl.grant(owner,FILE_READ|FILE_WRITE,20).unwrap();
  let mut backend=Memory{bytes:[0;16],len:0,corrupt:false};
  replace_and_verify(&acl,owner,2,"/workspace/file",b"ok",&mut backend).unwrap();
  let mut buf=[0;16];
  assert_eq!(read_authorized(&acl,Identity{generation:8,..owner},
    2,&mut backend,"/workspace/file",&mut buf),
    Err(Error::Unauthorized));
  assert_eq!(read_authorized(&acl,owner,2,&mut backend,
    "/workspace/file",&mut buf),Ok(2));
  backend.corrupt=true;
  assert_eq!(replace_and_verify(&acl,owner,2,"/workspace/file",
    b"new",&mut backend),Err(Error::Corrupt));
 }
}
