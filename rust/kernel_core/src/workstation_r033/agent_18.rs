//! R033/18: Debugger breakpoint set limits. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn unique(pcs:&[u64])->Result<(),Error>{if pcs.len()>64{return Err(Error::Full);}for(i,pc)in pcs.iter().enumerate(){if *pc==0||pcs[..i].contains(pc){return Err(Error::Invalid);}}Ok(())}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(unique(&[10,20]),Ok(()));assert_eq!(unique(&[10,10]),Err(Error::Invalid));}}
