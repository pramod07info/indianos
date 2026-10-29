//! R033/04: Project file size quota guard. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn admit<const F:usize>(fs:&RamFs<F>,path:&[u8],additional:usize,quota:usize)->Result<(),Error>{let (_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;if n.checked_add(additional).is_none_or(|x|x>quota||x>CONTENT_MAX){Err(Error::Full)}else{Ok(())}}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"1234").unwrap();assert_eq!(admit(&fs,b"/p/a",2,5),Err(Error::Full));assert_eq!(admit(&fs,b"/p/a",1,5),Ok(()));}}
