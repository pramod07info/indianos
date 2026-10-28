//! R023/04: bracket matching/navigation over source-buffer bytes.
//! Scans quoted strings and escaped characters; not a full Rust parser.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Pair{pub open:usize,pub close:usize,pub depth:u16}
fn kind(b:u8)->Option<(u8,bool)>{
 match b{b'('=>Some((0,true)),b')'=>Some((0,false)),
 b'['=>Some((1,true)),b']'=>Some((1,false)),
 b'{'=>Some((2,true)),b'}'=>Some((2,false)),_=>None}
}
pub fn matching(data:&[u8],cursor:usize)->Result<Pair,Error>{
 if cursor>=data.len(){return Err(Error::Invalid);}
 let (target,opening)=kind(data[cursor]).ok_or(Error::Invalid)?;
 let mut stack=[(0usize,0u8);256];let mut n=0usize;
 let mut quoted=false;let mut escaped=false;
 for (i,&b) in data.iter().enumerate(){
  if escaped{escaped=false;continue;}
  if quoted&&b==b'\\'{escaped=true;continue;}
  if b==b'"'{quoted=!quoted;continue;}
  if quoted{continue;}
  if let Some((k,open))=kind(b){
   if open{
    if n==stack.len(){return Err(Error::Full);}
    stack[n]=(i,k);n+=1;
   }else{
    if n==0{return Err(Error::Conflict);}
    n-=1;
    let (first,kind_open)=stack[n];
    if kind_open!=k{return Err(Error::Conflict);}
    if k==target&&(opening&&first==cursor||!opening&&i==cursor){
     return Ok(Pair{open:first,close:i,depth:n as u16+1});
    }
   }
  }
 }
 Err(Error::Missing)
}
pub fn in_file<const F:usize>(fs:&RamFs<F>,path:&[u8],
 cursor:usize)->Result<Pair,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut bytes=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut bytes[..size]).map_err(|_|Error::Missing)?;
 matching(&bytes[..size],cursor)
}
pub fn balanced(data:&[u8])->bool{
 let mut stack=[0u8;256];let mut depth=0usize;let mut quote=false;let mut escaped=false;
 for &b in data{
  if escaped{escaped=false;continue;}
  if quote&&b==b'\\'{escaped=true;continue;}
  if b==b'"'{quote=!quote;continue;}
  if quote{continue;}
  if let Some((k,open))=kind(b){
   if open{if depth>=256{return false;}stack[depth]=k;depth+=1;}
   else{if depth==0||stack[depth-1]!=k{return false;}depth-=1;}
  }
 }depth==0&&!quote
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn nested_real_source_brackets_match_positions(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"fn x(){call(1)}").unwrap();
  let outer=in_file(&fs,b"/a",6).unwrap();
  assert_eq!((outer.open,outer.close),(6,14));
  let inner=in_file(&fs,b"/a",11).unwrap();
  assert_eq!((inner.open,inner.close),(11,13));
 }
 #[test]fn quoted_brackets_are_ignored_by_scanner(){
  assert!(balanced(b"fn x(){ print(\"}\"); }"));
  assert_eq!(matching(b"\"}\"{}",3).unwrap().open,3);
 }
 #[test]fn mismatched_brackets_are_rejected_as_invalid_source(){
  assert!(!balanced(b"{[})"));
  assert_eq!(matching(b"{[})",0),Err(Error::Conflict));
 }
}
