//! R036/16: Trusted test evidence tied to actual source bytes.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn results<const F:usize>(fs:&RamFs<F>,source:&[u8],trusted:bool,passed:usize,failed:usize,expected_generation:u64)->Result<bool,Error>{if !trusted{return Err(Error::Denied);}let(rev,_)=fs.metadata(source).map_err(|_|Error::Missing)?;if rev!=expected_generation{return Err(Error::Stale);}if passed==0&&failed==0{return Err(Error::Invalid);}Ok(passed>0&&failed==0)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<1>::new();fs.replace(b"/p/test.rs",b"test").unwrap();assert_eq!(results(&fs,b"/p/test.rs",true,3,0,1),Ok(true));assert_eq!(results(&fs,b"/p/test.rs",false,3,0,1),Err(Error::Denied));}}
