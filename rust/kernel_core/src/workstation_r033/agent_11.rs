//! R033/11: Terminal numeric argument parser. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn number(v:&[u8])->Result<u32,Error>{if v.is_empty()||v.len()>10{return Err(Error::Invalid);}let mut n=0u32;for &c in v{if !c.is_ascii_digit(){return Err(Error::Invalid);}n=n.checked_mul(10).and_then(|x|x.checked_add((c-b'0') as u32)).ok_or(Error::Overflow)?;}Ok(n)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(number(b"42"),Ok(42));assert_eq!(number(b"4294967296"),Err(Error::Overflow));}}
