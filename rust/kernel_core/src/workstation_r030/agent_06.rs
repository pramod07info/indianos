//! R030/06: exact-identifier source rename with optimistic revision safety.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
fn ident(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
fn name(s:&[u8])->bool{!s.is_empty()&&
 (s[0].is_ascii_alphabetic()||s[0]==b'_')&&s.iter().all(|x|ident(*x))}
pub fn rename<const F:usize>(fs:&mut RamFs<F>,path:&[u8],revision:u64,
 old:&[u8],new:&[u8])->Result<usize,Error>{
 if !name(old)||!name(new)||old==new{return Err(Error::Invalid);}
 let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..n])
  .map_err(|_|Error::Missing)?;
 let mut dst=[0u8;CONTENT_MAX];let(mut i,mut w,mut hits)=(0,0,0);
 while i<n{
  let matched=i+old.len()<=n&&&src[i..i+old.len()]==old&&
   (i==0||!ident(src[i-1]))&&
   (i+old.len()==n||!ident(src[i+old.len()]));
  let bytes=if matched{new}else{&src[i..i+1]};
  if w.checked_add(bytes.len()).is_none_or(|v|v>CONTENT_MAX){return Err(Error::Full);}
  dst[w..w+bytes.len()].copy_from_slice(bytes);w+=bytes.len();
  if matched{i+=old.len();hits+=1;}else{i+=1;}
 }
 if hits!=0{fs.replace(path,&dst[..w]).map_err(|_|Error::Full)?;}Ok(hits)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn identifier_rename_leaves_substrings_and_other_symbols_untouched(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a",b"let foo=foo; foobar _foo").unwrap();
  assert_eq!(rename(&mut fs,b"/p/a",1,b"foo",b"bar"),Ok(2));
  let mut out=[0u8;32];let n=fs.read(b"/p/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"let bar=bar; foobar _foo");
 }
 #[test]fn invalid_names_and_stale_revisions_do_not_mutate(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a",b"foo").unwrap();
  assert_eq!(rename(&mut fs,b"/p/a",1,b"foo",b"a b"),Err(Error::Invalid));
  assert_eq!(rename(&mut fs,b"/p/a",2,b"foo",b"bar"),Err(Error::Stale));
  assert_eq!(fs.version(),1);
 }
}