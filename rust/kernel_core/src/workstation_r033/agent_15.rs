//! R033/15: Build epoch monotonic sequence. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn advance(old:u64,next:u64)->Result<u64,Error>{if next<=old{return Err(Error::Stale);}Ok(next)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(advance(4,5),Ok(5));assert_eq!(advance(4,4),Err(Error::Stale));}}
