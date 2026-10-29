//! R027/08: source identifier rename preview using bounded exact-token matching.
//! Staged operation is lexical, not semantic language-server refactoring.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub start:usize,pub end:usize,pub line:u32}
pub const EMPTY:Hit=Hit{start:0,end:0,line:0};
fn word(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn find<const O:usize>(text:&[u8],symbol:&[u8],
 out:&mut[Hit;O])->Result<usize,Error>{
 if symbol.is_empty()||symbol.len()>64||!symbol.iter().all(|&b|word(b)){
  return Err(Error::Invalid);
 }
 let mut results=[EMPTY;O];let mut n=0;let mut line=1;
 if symbol.len()>text.len(){*out=results;return Ok(0);}
 for i in 0..=text.len()-symbol.len(){
  if &text[i..i+symbol.len()]==symbol&&
   (i==0||!word(text[i-1]))&&
   (i+symbol.len()==text.len()||!word(text[i+symbol.len()])){
   if n>=O{return Err(Error::Full);}
   results[n]=Hit{start:i,end:i+symbol.len(),line};n+=1;
  }
  if text[i]==b'\n'{line+=1;}
 }
 *out=results;Ok(n)
}
pub fn file_preview<const F:usize,const O:usize>(fs:&RamFs<F>,
 path:&[u8],symbol:&[u8],hits:&mut[Hit;O])->Result<usize,Error>{
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut data=[0;CONTENT_MAX];fs.read(path,0,&mut data[..len]).map_err(|_|Error::Missing)?;
 find(&data[..len],symbol,hits)
}
pub fn replacement_size(source_len:usize,old_len:usize,new_len:usize,
 count:usize)->Result<usize,Error>{
 source_len.checked_sub(old_len.checked_mul(count).ok_or(Error::Overflow)?)
  .and_then(|n|n.checked_add(new_len.checked_mul(count)?)).ok_or(Error::Overflow)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn exact_name_preview_excludes_identifiers_with_prefix(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a.rs",b"foo foo_extra foo").unwrap();
  let mut out=[EMPTY;3];assert_eq!(file_preview(&fs,b"/a.rs",b"foo",&mut out),Ok(2));
  assert_eq!(out[1].start,14);
 }
 #[test]fn invalid_identifier_is_rejected_without_publishing_hits(){
  let mut out=[Hit{line:44,..EMPTY};1];
  assert_eq!(find(b"a",b"bad name",&mut out),Err(Error::Invalid));
  assert_eq!(out[0].line,44);
 }
 #[test]fn bounded_result_count_keeps_previous_preview(){
  let mut out=[Hit{line:44,..EMPTY};1];
  assert_eq!(find(b"foo foo",b"foo",&mut out),Err(Error::Full));
  assert_eq!(out[0].line,44);
 }
}
