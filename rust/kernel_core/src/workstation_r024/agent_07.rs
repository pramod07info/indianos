//! R024/07: project-wide Rust function/type symbol search over real source files.
//! Reuses R021 lexical outline, not a compiler or full language server.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX};
use crate::workstation_r021::agent_04::{self as outline,Symbol};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ResultItem{pub path:[u8;PATH_MAX],pub len:usize,
 pub name:[u8;48],pub name_len:usize,pub line:usize,
 pub symbol_offset:usize}
pub const EMPTY:ResultItem=ResultItem{path:[0;PATH_MAX],len:0,
 name:[0;48],name_len:0,line:0,symbol_offset:0};
pub fn scan<const F:usize,const O:usize>(
 fs:&RamFs<F>,prefix:&[u8],query:&[u8],out:&mut[ResultItem;O])
 ->Result<usize,Error>{
 if prefix.first()!=Some(&b'/')||query.len()>48{return Err(Error::Invalid);}
 let mut stage=[EMPTY;O];let mut n=0;
 for index in 0..fs.count(){
  let mut name=[0u8;PATH_MAX];
  let (len,_,_)=fs.entry(index,&mut name).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !name[..len].starts_with(prefix)||!name[..len].ends_with(b".rs"){continue;}
  let mut symbols=[outline::EMPTY;32];
  let count=outline::from_file(fs,&name[..len],&mut symbols)
   .map_err(|_|Error::Invalid)?;
  for s in &symbols[..count]{
   if !s.name[..s.len].starts_with(query){continue;}
   if n==O{return Err(Error::Full);}
   stage[n]=ResultItem{path:name,len,name:s.name,name_len:s.len,
    line:s.line,symbol_offset:s.offset};n+=1;
  }
 }
 stage[..n].sort_unstable_by(|a,b|a.path[..a.len].cmp(&b.path[..b.len])
   .then(a.line.cmp(&b.line)));
 *out=stage;Ok(n)
}
pub fn count_named(items:&[ResultItem],name:&[u8])->usize{
 items.iter().filter(|x|&x.name[..x.name_len]==name).count()
}
pub fn first<'a>(items:&'a[ResultItem])->Option<&'a ResultItem>{items.first()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn editor_navigator_searches_functions_across_two_real_files(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/src/a.rs",b"pub fn alpha() {}\nfn beta() {}\n").unwrap();
  fs.replace(b"/src/b.rs",b"pub fn alps() {}\n").unwrap();
  let mut out=[EMPTY;4];
  assert_eq!(scan(&fs,b"/src",b"al",&mut out),Ok(2));
  assert_eq!(out[0].line,1);assert_eq!(&out[1].name[..out[1].name_len],b"alps");
 }
 #[test]fn non_source_files_do_not_pollute_rust_symbol_navigation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/src/x.txt",b"fn fake(){}\n").unwrap();
  let mut out=[EMPTY;2];assert_eq!(scan(&fs,b"/src",b"",&mut out),Ok(0));
 }
 #[test]fn overflow_preserves_previous_source_navigator_results(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/a.rs",b"fn first(){}\nfn second(){}\n").unwrap();
  let marker=ResultItem{line:99,..EMPTY};let mut out=[marker;1];
  assert_eq!(scan(&fs,b"/src",b"",&mut out),Err(Error::Full));
  assert_eq!(out,[marker]);
 }
}
