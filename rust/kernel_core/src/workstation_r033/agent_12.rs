//! R033/12: Terminal output bounded truncation. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn tail<'a>(data:&'a[u8],max:usize)->Result<&'a[u8],Error>{if max==0{return Err(Error::Invalid);}let mut start=data.len().saturating_sub(max);while start<data.len()&&start>0&&data[start-1]!=b'\n'{start+=1;}Ok(&data[start..])}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(tail(b"old\nlatest\n",7),Ok(b"latest\n".as_slice()));assert!(tail(b"abc",0).is_err());}}
