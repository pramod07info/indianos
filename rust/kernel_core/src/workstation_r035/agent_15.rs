//! R035/15 — Compiler artifact verification locked to source generation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn verify<const F:usize>(fs:&RamFs<F>,source:&[u8],source_rev:u64,bytes:&[u8],digest:u64)->Result<(),Error>{if bytes.is_empty(){return Err(Error::Invalid);}let(v,_)=fs.metadata(source).map_err(|_|Error::Missing)?;if v!=source_rev{return Err(Error::Stale);}if crate::workstation_r032::agent_02::digest(bytes)!=digest{return Err(Error::Conflict);}Ok(())}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"code").unwrap();let d=crate::workstation_r032::agent_02::digest(b"artifact");assert_eq!(verify(&fs,b"/p/a",1,b"artifact",d),Ok(()));assert_eq!(verify(&fs,b"/p/a",1,b"fake",d),Err(Error::Conflict));}}
