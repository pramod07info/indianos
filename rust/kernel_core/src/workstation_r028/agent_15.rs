//! R028/15: lexical Rust test and assertion inventory over live project files.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Counts{pub path:[u8;PATH_MAX],pub len:usize,pub tests:usize,
 pub asserts:usize,pub revision:u64}
pub const EMPTY:Counts=Counts{path:[0;PATH_MAX],len:0,tests:0,asserts:0,revision:0};
fn occurrences(bytes:&[u8],needle:&[u8])->usize{
 bytes.windows(needle.len()).filter(|w|*w==needle).count()
}
pub fn inspect<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<Counts,Error>{
 if path.len()<2||path.len()>PATH_MAX{return Err(Error::Invalid);}
 let(revision,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..len])
  .map_err(|_|Error::Missing)?;
 let mut name=[0u8;PATH_MAX];name[..path.len()].copy_from_slice(path);
 Ok(Counts{path:name,len:path.len(),tests:occurrences(&src[..len],b"#[test]"),
  asserts:occurrences(&src[..len],b"assert!")+occurrences(&src[..len],b"assert_eq!"),
  revision})
}
pub fn project<const F:usize,const N:usize>(fs:&RamFs<F>,
 root:&[u8],out:&mut[Counts;N])->Result<usize,Error>{
 if root.is_empty()||root[0]!=b'/'{return Err(Error::Invalid);}
 let mut stage=[EMPTY;N];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];let(n,_,_)=fs.entry(i,&mut path)
   .map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..n].starts_with(root)||!path[..n].ends_with(b".rs"){continue;}
  if count==N{return Err(Error::Full);}
  stage[count]=inspect(fs,&path[..n])?;count+=1;
 }
 *out=stage;Ok(count)
}
pub fn total_tests(index:&[Counts])->usize{index.iter().map(|x|x.tests).sum()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn count_real_rust_unit_tests_and_assertions(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/main.rs",b"#[test]\nfn a(){assert!(true);}\n").unwrap();
  let mut out=[EMPTY;1];assert_eq!(project(&fs,b"/p",&mut out),Ok(1));
  assert_eq!((out[0].tests,out[0].asserts),(1,1));
 }
 #[test]fn insufficient_capacity_does_not_publish_partial_testing_index(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a.rs",b"#[test]").unwrap();fs.replace(b"/p/b.rs",b"#[test]").unwrap();
  let mut out=[Counts{tests:77,..EMPTY};1];
  assert_eq!(project(&fs,b"/p",&mut out),Err(Error::Full));
  assert_eq!(out[0].tests,77);
 }
}