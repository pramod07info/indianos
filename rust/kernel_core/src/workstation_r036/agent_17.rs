//! R036/17: Debugger line-to-PC breakpoint validation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn breakpoint<const F:usize>(fs:&RamFs<F>,source:&[u8],revision:u64,line:u32,pc:u64,event_line:u32,event_pc:u64,trusted:bool)->Result<bool,Error>{if !trusted{return Err(Error::Denied);}let(rev,_)=fs.metadata(source).map_err(|_|Error::Missing)?;if rev!=revision{return Err(Error::Stale);}if line==0||pc==0{return Err(Error::Invalid);}Ok(line==event_line&&pc==event_pc)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"fn a(){}").unwrap();assert_eq!(breakpoint(&fs,b"/p/a.rs",1,1,4096,1,4096,true),Ok(true));}}
