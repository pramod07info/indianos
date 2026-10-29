//! R033/10: Project path root guard. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn guard(root:&[u8],path:&[u8])->Result<(),Error>{if root.len()<2||!root.starts_with(b"/")||!path.starts_with(root)||path.get(root.len())!=Some(&b'/')||path.windows(2).any(|x|x==b"..")||path.contains(&0){return Err(Error::Denied);}Ok(())}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(guard(b"/p",b"/p/src/a"),Ok(()));assert_eq!(guard(b"/p",b"/private/a"),Err(Error::Denied));assert_eq!(guard(b"/p",b"/p/../q"),Err(Error::Denied));}}
