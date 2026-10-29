//! R035/17 — Source-revision guarded breakpoint control.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn hit<const F:usize>(fs:&RamFs<F>,path:&[u8],expected:u64,source_line:u32,pc:u64,event_line:u32,event_pc:u64,trusted:bool)->Result<bool,Error>{if !trusted{return Err(Error::Denied);}let(v,_)=fs.metadata(path).map_err(|_|Error::Missing)?;if v!=expected{return Err(Error::Stale);}if source_line==0||pc==0{return Err(Error::Invalid);}Ok(source_line==event_line&&pc==event_pc)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_workflow_and_failure_guard(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"fn main(){}").unwrap();assert_eq!(hit(&fs,b"/p/a",1,1,4096,1,4096,true),Ok(true));assert_eq!(hit(&fs,b"/p/a",1,1,4096,1,4096,false),Err(Error::Denied));}}
