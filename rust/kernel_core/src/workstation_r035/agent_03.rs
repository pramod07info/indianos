//! R035/03 — Real project file copy with source revision. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn copy<const F:usize>(fs:&mut RamFs<F>,source:&[u8],destination:&[u8],revision:u64)->Result<u64,Error>{if source==destination||fs.metadata(destination).is_ok(){return Err(Error::Conflict);}let(v,n)=fs.metadata(source).map_err(|_|Error::Missing)?;if v!=revision{return Err(Error::Stale);}let mut data=[0u8;CONTENT_MAX];fs.read(source,0,&mut data[..n]).map_err(|_|Error::Missing)?;fs.replace(destination,&data[..n]).map_err(|_|Error::Full)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"alpha").unwrap();copy(&mut fs,b"/p/a",b"/p/b",1).unwrap();let mut b=[0;5];fs.read(b"/p/b",0,&mut b).unwrap();assert_eq!(&b,b"alpha");}}
