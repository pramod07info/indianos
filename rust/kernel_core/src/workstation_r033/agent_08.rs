//! R033/08: Undo-stack checked history. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub struct History<const N:usize>{values:[u64;N],len:usize}impl<const N:usize> History<N>{pub fn new()->Self{Self{values:[0;N],len:0}}pub fn push(&mut self,v:u64)->Result<(),Error>{if self.len==N{return Err(Error::Full);}self.values[self.len]=v;self.len+=1;Ok(())}pub fn undo(&mut self)->Result<u64,Error>{if self.len==0{return Err(Error::Missing);}self.len-=1;Ok(self.values[self.len])}}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){let mut h=History::<2>::new();h.push(1).unwrap();h.push(2).unwrap();assert_eq!(h.undo(),Ok(2));assert_eq!(h.undo(),Ok(1));assert_eq!(h.undo(),Err(Error::Missing));}}
