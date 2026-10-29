//! R033/19: Developer workspace pane size validation. Real bounded kernel-side workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn pane(width:u16,height:u16,min_w:u16,min_h:u16)->Result<u32,Error>{if width<min_w||height<min_h||min_w==0||min_h==0{return Err(Error::Full);}Ok((width as u32)*(height as u32))}
#[cfg(test)]mod tests{use super::*;#[test]fn functionality_and_failure_guards(){assert_eq!(pane(800,600,200,100),Ok(480000));assert_eq!(pane(100,600,200,100),Err(Error::Full));}}
