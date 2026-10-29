//! R035/02 — Atomic source create and collision guard. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn create<const F:usize>(fs:&mut RamFs<F>,path:&[u8],body:&[u8],expected_generation:u64)->Result<u64,Error>{if fs.version()!=expected_generation{return Err(Error::Stale);}if fs.metadata(path).is_ok(){return Err(Error::Conflict);}if body.len()>CONTENT_MAX{return Err(Error::Full);}fs.replace(path,body).map_err(|_|Error::Full)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<2>::new();assert_eq!(create(&mut fs,b"/p/a",b"fn main() {}",0),Ok(1));assert_eq!(create(&mut fs,b"/p/a",b"new",1),Err(Error::Conflict));}}
