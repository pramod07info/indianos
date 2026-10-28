//! R019/01: real RAM filesystem-backed project file transactions.
//! Signed-in caller rights and global revision are verified before mutation.
//! Durable block storage will require a separate proven backend.
use super::Error;
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,FILE_READ,FILE_WRITE};
use crate::batch6_storage::AtomicFileBackend;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
fn permitted<const G:usize>(auth:&Access<G>,actor:Identity,right:u32,now:u64)
 ->Result<(),Error>{auth.require(actor,right,now).map_err(|_|Error::Denied)}
pub fn save<const N:usize,const G:usize>(fs:&mut RamFs<N>,auth:&Access<G>,
 actor:Identity,now:u64,path:&[u8],bytes:&[u8],rev:u64)->Result<u64,Error>{
 permitted(auth,actor,FILE_WRITE,now)?;
 fs.save_if_revision(path,bytes,rev).map_err(|_|Error::Conflict)
}
pub fn append<const N:usize,const G:usize>(fs:&mut RamFs<N>,auth:&Access<G>,
 actor:Identity,now:u64,path:&[u8],bytes:&[u8],rev:u64)->Result<u64,Error>{
 permitted(auth,actor,FILE_WRITE,now)?;
 fs.append_if_revision(path,bytes,rev).map_err(|_|Error::Conflict)
}
pub fn read_exact<const N:usize,const G:usize>(fs:&mut RamFs<N>,auth:&Access<G>,
 actor:Identity,now:u64,path:&[u8],out:&mut[u8])->Result<usize,Error>{
 permitted(auth,actor,FILE_READ,now)?;
 fs.read_all(path,out).map_err(|_|Error::Full)
}
pub fn content_hash<const N:usize,const G:usize>(fs:&mut RamFs<N>,
 auth:&Access<G>,actor:Identity,now:u64,path:&[u8])->Result<u32,Error>{
 let mut bytes=[0u8;CONTENT_MAX];let n=read_exact(fs,auth,actor,now,path,&mut bytes)?;
 let mut h=0x811c9dc5u32;
 for &v in &bytes[..n]{h=(h^(v as u32)).wrapping_mul(0x01000193);}
 Ok(h)
}
#[cfg(test)] mod tests{
 use super::*;
 fn id(g:u64)->Identity{Identity{pid:10,generation:g}}
 fn acl()->Access<1>{let mut a=Access::new();a.grant(id(1),FILE_READ|FILE_WRITE,100).unwrap();a}
 #[test]fn actual_project_edits_check_revision_and_bytes(){
  let mut fs=RamFs::<2>::new();
  assert_eq!(save(&mut fs,&acl(),id(1),1,b"/src/a.rs",b"mod a;",0),Ok(1));
  assert_eq!(append(&mut fs,&acl(),id(1),1,b"/src/a.rs",b"\n",1),Ok(2));
  let mut out=[0u8;20];assert_eq!(read_exact(&mut fs,&acl(),id(1),1,b"/src/a.rs",&mut out),Ok(7));
  assert_eq!(&out[..7],b"mod a;\n");assert_ne!(content_hash(&mut fs,&acl(),id(1),1,b"/src/a.rs").unwrap(),0);
 }
 #[test]fn stale_editor_and_recycled_process_are_denied(){
  let mut fs=RamFs::<1>::new();save(&mut fs,&acl(),id(1),1,b"/a",b"new",0).unwrap();
  assert_eq!(save(&mut fs,&acl(),id(1),1,b"/a",b"old",0),Err(Error::Conflict));
  assert_eq!(save(&mut fs,&acl(),id(2),1,b"/a",b"x",1),Err(Error::Denied));
  let mut out=[77u8;4];
  assert_eq!(read_exact(&mut fs,&acl(),id(2),1,b"/a",&mut out),Err(Error::Denied));
  assert_eq!(out,[77;4]);
 }
 #[test]fn full_buffer_read_is_fail_atomic(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abcdef").unwrap();
  let mut out=[55u8;2];
  assert_eq!(read_exact(&mut fs,&acl(),id(1),1,b"/a",&mut out),Err(Error::Full));
  assert_eq!(out,[55;2]);
 }
}
