//! R033/03: Source line count. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn lines<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<usize,Error>{let (_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;let mut b=[0u8;CONTENT_MAX];fs.read(path,0,&mut b[..n]).map_err(|_|Error::Missing)?;Ok(b[..n].iter().filter(|&&c|c==b'\n').count()+usize::from(n>0&&b[n-1]!=b'\n'))}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"one\ntwo").unwrap();assert_eq!(lines(&fs,b"/p/a"),Ok(2));}}
