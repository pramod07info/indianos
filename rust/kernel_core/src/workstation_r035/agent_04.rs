//! R035/04 — Real file rename and stale version guard. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn rename<const F:usize>(fs:&mut RamFs<F>,from:&[u8],to:&[u8],expected:u64)->Result<u64,Error>{let(v,_)=fs.metadata(from).map_err(|_|Error::Missing)?;if v!=expected{return Err(Error::Stale);}fs.rename_if_version(from,to,expected).map_err(|_|Error::Conflict)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<2>::new();fs.replace(b"/p/a",b"a").unwrap();assert_eq!(rename(&mut fs,b"/p/a",b"/p/b",0),Err(Error::Stale));assert!(rename(&mut fs,b"/p/a",b"/p/b",1).is_ok());}}
