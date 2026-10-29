//! R033/17: Debugger memory watch monotonic events. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct Watch{pub pc:u64,pub generation:u64}pub fn update(w:&mut Watch,pc:u64,generation:u64)->Result<bool,Error>{if pc==0||generation<=w.generation{return Err(Error::Stale);}let changed=w.pc!=pc;w.pc=pc;w.generation=generation;Ok(changed)}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut w=Watch{pc:10,generation:1};assert_eq!(update(&mut w,11,2),Ok(true));assert_eq!(update(&mut w,11,2),Err(Error::Stale));}}
