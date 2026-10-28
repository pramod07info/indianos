//! R025/08 cross-file lexical symbol references from real project Rust source.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Link{pub path:[u8;PATH_MAX],pub len:usize,
 pub offset:usize,pub line:usize,pub revision:u64}
pub const EMPTY:Link=Link{path:[0;PATH_MAX],len:0,offset:0,line:0,revision:0};
fn ident(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn references<const F:usize,const O:usize>(fs:&RamFs<F>,
 prefix:&[u8],symbol:&[u8],out:&mut[Link;O])->Result<usize,Error>{
 if prefix.first()!=Some(&b'/')||symbol.is_empty()||symbol.len()>64||
  !symbol.iter().all(|&b|ident(b)){return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut count=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(prefix)||!path[..len].ends_with(b".rs"){continue;}
  let mut bytes=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut bytes[..size])
   .map_err(|_|Error::Missing)?;
  if symbol.len()>size{continue;}
  let mut line=1usize;
  for pos in 0..=size-symbol.len(){
   if &bytes[pos..pos+symbol.len()]==symbol&&
    (pos==0||!ident(bytes[pos-1]))&&
    (pos+symbol.len()==size||!ident(bytes[pos+symbol.len()])){
    if count==O{return Err(Error::Full);}
    stage[count]=Link{path,len,offset:pos,line,revision};count+=1;
   }
   if bytes[pos]==b'\n'{line+=1;}
  }
 }
 stage[..count].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len])
  .then(a.offset.cmp(&b.offset)));
 *out=stage;Ok(count)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,link:Link)->bool{
 fs.metadata(&link.path[..link.len]).is_ok_and(|(v,_)|v==link.revision)
}
pub fn count_file(links:&[Link],path:&[u8])->usize{
 links.iter().filter(|x|&x.path[..x.len]==path).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_project_functions_are_referenced_across_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/a.rs",b"fn hello() {}\n").unwrap();
  fs.replace(b"/p/b.rs",b"hello();\nhello();\n").unwrap();
  let mut out=[EMPTY;4];
  assert_eq!(references(&fs,b"/p",b"hello",&mut out),Ok(3));
  assert_eq!(out[2].line,2);
 }
 #[test]fn longer_identifier_not_counted_as_exact_reference(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a.rs",b"hello_world hello").unwrap();
  let mut out=[EMPTY;2];assert_eq!(references(&fs,b"/",b"hello",&mut out),Ok(1));
 }
 #[test]fn capacity_failure_is_atomic_for_symbol_index(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"x x x").unwrap();
  let mut out=[Link{offset:77,..EMPTY};1];
  assert_eq!(references(&fs,b"/",b"x",&mut out),Err(Error::Full));
  assert_eq!(out[0].offset,77);
 }
}
