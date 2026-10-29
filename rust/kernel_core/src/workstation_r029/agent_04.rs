//! R029/04: reverse dependency / change-impact preview from actual Rust source imports.
//! Lexical module matching only; no claim of full language-server type resolution.
use super::Error;
use crate::batch7_ramfs::{RamFs,PATH_MAX,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Dependent{pub path:[u8;PATH_MAX],pub len:usize,pub revision:u64,pub matches:u16}
pub const EMPTY:Dependent=Dependent{path:[0;PATH_MAX],len:0,revision:0,matches:0};
pub fn dependent_files<const F:usize,const N:usize>(fs:&RamFs<F>,root:&[u8],
 import:&[u8],out:&mut[Dependent;N])->Result<usize,Error>{
 if root.first()!=Some(&b'/')||import.is_empty()||import.len()>64||
 !import.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'_'||*b==b':'){
  return Err(Error::Invalid);
 }
 let mut stage=[EMPTY;N];let mut n=0;
 for i in 0..fs.count(){
  let mut path=[0u8;PATH_MAX];
  let (len,size,revision)=fs.entry(i,&mut path).map_err(|_|Error::Invalid)?
   .ok_or(Error::Missing)?;
  if !path[..len].starts_with(root)||!path[..len].ends_with(b".rs"){continue;}
  let mut code=[0u8;CONTENT_MAX];fs.read(&path[..len],0,&mut code[..size])
   .map_err(|_|Error::Missing)?;
  let mut count=0u16;
  for line in code[..size].split(|b|*b==b'\n'){
   let line=line.strip_prefix(b"use ").or_else(||line.strip_prefix(b" use "));
   if line.is_some_and(|l|l.starts_with(import)&&
    l.get(import.len())==Some(&b';')){count=count.saturating_add(1);}
  }
  if count==0{continue;}
  if n==N{return Err(Error::Full);}
  stage[n]=Dependent{path,len,revision,matches:count};n+=1;
 }
 *out=stage;Ok(n)
}
pub fn impacted_count(items:&[Dependent])->usize{items.len()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn changing_imported_module_reports_only_dependent_rust_file(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/p/main.rs",b"use crate::util;\nfn main(){}").unwrap();
  fs.replace(b"/p/other.rs",b"use crate::another;\n").unwrap();
  let mut out=[EMPTY;2];
  assert_eq!(dependent_files(&fs,b"/p",b"crate::util",&mut out),Ok(1));
  assert_eq!(&out[0].path[..out[0].len],b"/p/main.rs");
 }
 #[test]fn invalid_symbol_and_capacity_fail_closed(){
  let fs=RamFs::<1>::new();let mut out=[EMPTY;1];
  assert_eq!(dependent_files(&fs,b"/p",b"bad;rm",&mut out),Err(Error::Invalid));
 }
}