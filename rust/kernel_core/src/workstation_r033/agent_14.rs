//! R033/14: Build dependency ready mask. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn ready(dependencies:u32,completed:u32,failed:u32)->Result<bool,Error>{if dependencies&failed!=0{return Err(Error::Conflict);}Ok(dependencies&!completed==0)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(ready(3,3,0),Ok(true));assert_eq!(ready(3,1,0),Ok(false));assert_eq!(ready(3,1,2),Err(Error::Conflict));}}
