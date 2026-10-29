//! R033/09: Source word frequency. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn count(haystack:&[u8],needle:&[u8])->Result<usize,Error>{if needle.is_empty(){return Err(Error::Invalid);}Ok(haystack.windows(needle.len()).filter(|x|*x==needle).count())}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(count(b"abcabc",b"abc"),Ok(2));assert_eq!(count(b"abc",b""),Err(Error::Invalid));}}
