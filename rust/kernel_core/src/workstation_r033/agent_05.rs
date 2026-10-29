//! R033/05: Real file newline normalization. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn normalize<const F:usize>(fs:&mut RamFs<F>,path:&[u8],rev:u64)->Result<usize,Error>{let (actual,n)=fs.metadata(path).map_err(|_|Error::Missing)?;if actual!=rev{return Err(Error::Stale);}let mut b=[0u8;CONTENT_MAX];fs.read(path,0,&mut b[..n]).map_err(|_|Error::Missing)?;let(mut w,mut changed)=(0usize,0usize);for i in 0..n{if b[i]==b'\r'&&i+1<n&&b[i+1]==b'\n'{changed+=1;continue;}b[w]=b[i];w+=1;}if changed>0{fs.replace(path,&b[..w]).map_err(|_|Error::Full)?;}Ok(changed)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"a\r\nb").unwrap();assert_eq!(normalize(&mut fs,b"/p/a",1),Ok(1));let mut b=[0;3];fs.read(b"/p/a",0,&mut b).unwrap();assert_eq!(&b,b"a\nb");}}
