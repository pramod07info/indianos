//! R033/16: Test suite outcome aggregate. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn passed(results:&[(u32,bool)])->Result<bool,Error>{if results.is_empty(){return Err(Error::Missing);}let mut seen=0u32;for &(id,ok) in results{if id>=32||seen&(1<<id)!=0{return Err(Error::Invalid);}seen|=1<<id;if !ok{return Ok(false);}}Ok(true)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(passed(&[(1,true),(2,true)]),Ok(true));assert_eq!(passed(&[(1,true),(2,false)]),Ok(false));}}
