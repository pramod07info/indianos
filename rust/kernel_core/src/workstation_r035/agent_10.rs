//! R035/10 — Actual source search results with byte offsets. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn search<const F:usize,const N:usize>(fs:&RamFs<F>,path:&[u8],query:&[u8],out:&mut[usize;N])->Result<usize,Error>{if query.is_empty(){return Err(Error::Invalid);}let(_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..n]).map_err(|_|Error::Missing)?;let mut stage=[0usize;N];let mut hits=0;for(i,v)in src[..n].windows(query.len()).enumerate(){if v==query{if hits==N{return Err(Error::Full);}stage[hits]=i;hits+=1;}}*out=stage;Ok(hits)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"foo foo").unwrap();let mut out=[0;2];assert_eq!(search(&fs,b"/p/a",b"foo",&mut out),Ok(2));assert_eq!(out,[0,4]);}}
