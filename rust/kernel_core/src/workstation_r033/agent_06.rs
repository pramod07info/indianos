//! R033/06: Editor tab ordinal pagination. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn page(count:usize,start:usize,capacity:usize)->Result<(usize,usize),Error>{if capacity==0{return Err(Error::Invalid);}let end=start.saturating_add(capacity).min(count);Ok((start.min(count),end))}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(page(12,5,4),Ok((5,9)));assert_eq!(page(12,5,0),Err(Error::Invalid));}}
