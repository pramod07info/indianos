//! R033/07: Safe UTF-8 byte-to-character cursor. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn cursor(s:&[u8],index:usize)->Result<usize,Error>{let text=core::str::from_utf8(s).map_err(|_|Error::Invalid)?;if index>text.len()||!text.is_char_boundary(index){return Err(Error::Invalid);}Ok(text[..index].chars().count())}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(cursor("αb".as_bytes(),2),Ok(1));assert_eq!(cursor("αb".as_bytes(),1),Err(Error::Invalid));}}
