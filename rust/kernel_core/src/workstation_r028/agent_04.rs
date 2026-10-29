//! R028/04: bounded Rust use-path dependency extraction over real project source.
//! Lexical map: not macro expansion or compiler-resolved module semantics.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Import{pub source:[u8;PATH_MAX],pub source_len:usize,
 pub target:[u8;96],pub target_len:usize,pub line:usize,pub revision:u64}
pub const EMPTY:Import=Import{source:[0;PATH_MAX],source_len:0,
 target:[0;96],target_len:0,line:0,revision:0};
pub fn extract<const F:usize,const O:usize>(fs:&RamFs<F>,root:&[u8],
 out:&mut[Import;O])->Result<usize,Error>{
 if root.is_empty()||root[0]!=b'/'{return Err(Error::Invalid);}
 let mut entries=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let(len,size,rev)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?.ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||!path[..len].ends_with(b".rs"){continue;}
  let mut source=[0u8;CONTENT_MAX];
  fs.read(&path[..len],0,&mut source[..size]).map_err(|_|Error::Missing)?;
  for (line,chunk) in source[..size].split(|&b|b==b'\n').enumerate(){
   let s=&chunk[chunk.iter().take_while(|&&b|b==b' ').count()..];
   if !s.starts_with(b"use "){continue;}
   let rest=&s[4..];let n=rest.iter().position(|&b|b==b';').ok_or(Error::Invalid)?;
   if n==0||n>96{return Err(Error::Invalid);}
   if count==O{return Err(Error::Full);}
   let mut item=EMPTY;item.source=path;item.source_len=len;
   item.target[..n].copy_from_slice(&rest[..n]);item.target_len=n;
   item.line=line+1;item.revision=rev;entries[count]=item;count+=1;
  }
 }
 *out=entries;Ok(count)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,item:Import)->bool{
 fs.metadata(&item.source[..item.source_len]).is_ok_and(|(v,_)|v==item.revision)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn source_import_map_finds_two_real_rust_dependencies(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/p/a.rs",b"use crate::one;\n  use std::fmt;\nfn x(){}\n").unwrap();
  let mut out=[EMPTY;2];assert_eq!(extract(&fs,b"/p",&mut out),Ok(2));
  assert_eq!(&out[1].target[..out[1].target_len],b"std::fmt");
 }
 #[test]fn edited_source_invalidates_old_dependency_navigation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/a.rs",b"use a;\n").unwrap();
  let mut out=[EMPTY;1];extract(&fs,b"/p",&mut out).unwrap();
  fs.replace(b"/p/a.rs",b"use b;\n").unwrap();assert!(!valid(&fs,out[0]));
 }
}