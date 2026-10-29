//! R033/01: Project file deletion with revision protection. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn delete<const F:usize>(fs:&mut RamFs<F>,path:&[u8],expected:u64)->Result<u64,Error>{let (rev,_)=fs.metadata(path).map_err(|_|Error::Missing)?;if rev!=expected{return Err(Error::Stale);}fs.delete_if_version(path,rev).map_err(|_|Error::Conflict)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a").unwrap();assert!(delete(&mut fs,b"/p/a",0).is_err());assert!(delete(&mut fs,b"/p/a",1).is_ok());}}
