//! R035/13 — Real source diagnostic line and column validation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn validate<const F:usize>(fs:&RamFs<F>,path:&[u8],revision:u64,line:usize,column:usize)->Result<usize,Error>{let(v,n)=fs.metadata(path).map_err(|_|Error::Missing)?;if v!=revision{return Err(Error::Stale);}if line==0||column==0{return Err(Error::Invalid);}let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..n]).map_err(|_|Error::Missing)?;let mut start=0;for(i,l)in bytes[..n].split(|b|*b==b'\n').enumerate(){if i+1==line{if column>l.len()+1{return Err(Error::Invalid);}return Ok(start+column-1);}start+=l.len()+1;}Err(Error::Missing)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"foo\nbar").unwrap();assert_eq!(validate(&fs,b"/p/a",1,2,2),Ok(5));assert_eq!(validate(&fs,b"/p/a",1,2,9),Err(Error::Invalid));}}
