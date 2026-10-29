//! R033/13: Terminal job elapsed-duration bounds. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn duration(start:u64,end:u64,limit:u64)->Result<u64,Error>{let elapsed=end.checked_sub(start).ok_or(Error::Stale)?;if elapsed>limit{Err(Error::Full)}else{Ok(elapsed)}}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(duration(10,22,15),Ok(12));assert_eq!(duration(10,22,5),Err(Error::Full));}}
