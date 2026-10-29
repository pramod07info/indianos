//! R030/05: optimistic in-place find/replace of actual RAM-backed source.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn replace_all<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 revision:u64,needle:&[u8],replacement:&[u8])->Result<(usize,u64),Error>{
 if needle.is_empty()||needle.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..n])
  .map_err(|_|Error::Missing)?;
 let mut staged=[0u8;CONTENT_MAX];let (mut i,mut p,mut changes)=(0usize,0usize,0usize);
 while i<n{
  let matched=i+needle.len()<=n&&&src[i..i+needle.len()]==needle;
  let next=if matched{replacement}else{&src[i..i+1]};
  if p.checked_add(next.len()).is_none_or(|k|k>CONTENT_MAX){return Err(Error::Full);}
  staged[p..p+next.len()].copy_from_slice(next);p+=next.len();
  if matched{i+=needle.len();changes+=1;}else{i+=1;}
 }
 if changes==0{return Ok((0,revision));}
 let new_rev=fs.replace(path,&staged[..p]).map_err(|_|Error::Full)?;
 Ok((changes,new_rev))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn replace_all_updates_live_editor_file_atomically(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"foo foo").unwrap();
  assert_eq!(replace_all(&mut fs,b"/p/a",1,b"foo",b"x"),Ok((2,2)));
  let mut out=[0;3];fs.read(b"/p/a",0,&mut out).unwrap();assert_eq!(&out,b"x x");
 }
 #[test]fn oversized_result_does_not_publish_any_byte(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",&[b'a';CONTENT_MAX]).unwrap();
  assert_eq!(replace_all(&mut fs,b"/p/a",1,b"a",b"bb"),Err(Error::Full));
  assert_eq!(fs.version(),1);
 }
}