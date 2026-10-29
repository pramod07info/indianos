//! R035/09 — Actual guest file navigation and listing. Kernel-side implementation over actual RamFs/state.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn list<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],out:&mut[[u8;96];N])->Result<usize,Error>{let mut staged=[[0u8;96];N];let mut count=0;for i in 0..fs.count(){let mut name=[0u8;96];let(n,_,_)=fs.entry(i,&mut name).map_err(|_|Error::Missing)?.ok_or(Error::Missing)?;if name[..n].starts_with(root)&&name.get(root.len())==Some(&b'/'){if count==N{return Err(Error::Full);}staged[count]=name;count+=1;}}staged[..count].sort_unstable();*out=staged;Ok(count)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<2>::new();fs.replace(b"/p/b",b"b").unwrap();fs.replace(b"/p/a",b"a").unwrap();let mut names=[[0;96];2];assert_eq!(list(&fs,b"/p",&mut names),Ok(2));assert_eq!(&names[0][..4],b"/p/a");}}
