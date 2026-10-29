//! R036/03: Revision-safe file rename with root containment. Real guest filesystem/workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn rename<const F:usize>(fs:&mut RamFs<F>,root:&[u8],from:&[u8],to:&[u8],expected:u64)->Result<u64,Error>{for path in [from,to]{if !path.starts_with(root)||path.get(root.len())!=Some(&b'/')||path.windows(2).any(|p|p==b".."){return Err(Error::Denied);}}let (rev,_)=fs.metadata(from).map_err(|_|Error::Missing)?;if rev!=expected{return Err(Error::Stale);}if fs.metadata(to).is_ok(){return Err(Error::Conflict);}fs.rename_if_version(from,to,rev).map_err(|_|Error::Invalid)}
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<2>::new();fs.replace(b"/p/old",b"code").unwrap();assert!(rename(&mut fs,b"/p",b"/p/old",b"/p/new",1).is_ok());assert_eq!(fs.metadata(b"/p/old"),Err(crate::batch7_ramfs::Error::NotFound));}}
