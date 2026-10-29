//! R035/05 — Revision-bound editor full-buffer save. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn save<const F:usize>(fs:&mut RamFs<F>,path:&[u8],expected:u64,contents:&[u8])->Result<u64,Error>{let(v,_)=fs.metadata(path).map_err(|_|Error::Missing)?;if v!=expected{return Err(Error::Stale);}if contents.len()>CONTENT_MAX||core::str::from_utf8(contents).is_err(){return Err(Error::Invalid);}fs.replace(path,contents).map_err(|_|Error::Full)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"old").unwrap();assert_eq!(save(&mut fs,b"/p/a",1,b"new"),Ok(2));assert_eq!(save(&mut fs,b"/p/a",1,b"bad"),Err(Error::Stale));}}
