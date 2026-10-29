//! R033/02: Source byte checksum. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn checksum<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<u64,Error>{let (_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;let mut b=[0u8;CONTENT_MAX];fs.read(path,0,&mut b[..n]).map_err(|_|Error::Missing)?;Ok(b[..n].iter().fold(14695981039346656037u64,|a,c|(a^(*c as u64)).wrapping_mul(1099511628211)))}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"abc").unwrap();let v=checksum(&fs,b"/p/a").unwrap();fs.replace(b"/p/a",b"abd").unwrap();assert_ne!(checksum(&fs,b"/p/a"),Ok(v));}}
