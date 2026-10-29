//! R036/01: Workspace tree with stable revision identities. Real guest filesystem/workstation operation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
pub fn inventory<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],out:&mut[File;N])->Result<usize,Error>{if root.len()<2||root[0]!=b'/'{return Err(Error::Invalid);}let mut stage=[EMPTY;N];let mut count=0;for index in 0..fs.count(){let mut path=[0;PATH_MAX];let(len,size,revision)=fs.entry(index,&mut path).map_err(|_|Error::Missing)?.ok_or(Error::Missing)?;if !path[..len].starts_with(root)||path.get(root.len())!=Some(&b'/'){continue;}if count==N{return Err(Error::Full);}stage[count]=File{path,len,size,revision};count+=1;}stage[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len]));*out=stage;Ok(count)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct File{pub path:[u8;PATH_MAX],pub len:usize,pub size:usize,pub revision:u64}
pub const EMPTY:File=File{path:[0;PATH_MAX],len:0,size:0,revision:0};
#[cfg(test)]mod tests{use super::*;#[test]fn real_functional_regression(){let mut fs=RamFs::<3>::new();fs.replace(b"/p/b",b"BB").unwrap();fs.replace(b"/p/a",b"A").unwrap();fs.replace(b"/q/x",b"x").unwrap();let mut out=[EMPTY;3];assert_eq!(inventory(&fs,b"/p",&mut out),Ok(2));assert_eq!(&out[0].path[..out[0].len],b"/p/a");}}
