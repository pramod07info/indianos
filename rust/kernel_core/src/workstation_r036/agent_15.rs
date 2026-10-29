//! R036/15: Source-locked compiler artifact evidence.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn verify<const F:usize>(fs:&RamFs<F>,path:&[u8],revision:u64,artifact:&[u8],expected_hash:u64)->Result<(),Error>{let(v,_)=fs.metadata(path).map_err(|_|Error::Missing)?;if v!=revision{return Err(Error::Stale);}if artifact.is_empty(){return Err(Error::Invalid);}let hash=crate::workstation_r032::agent_02::digest(artifact);if hash!=expected_hash{return Err(Error::Conflict);}Ok(())}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/main.rs",b"code").unwrap();let hash=crate::workstation_r032::agent_02::digest(b"binary");assert_eq!(verify(&fs,b"/p/main.rs",1,b"binary",hash),Ok(()));}}
