//! R028/08: token-prefix suggestions from actual source with bounded distinct names.
//! Lexical completion, not a semantic compiler language server.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Candidate{pub bytes:[u8;48],pub len:usize,pub count:u16}
pub const EMPTY:Candidate=Candidate{bytes:[0;48],len:0,count:0};
fn word(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn index<const F:usize,const O:usize>(fs:&RamFs<F>,path:&[u8],
 prefix:&[u8],out:&mut[Candidate;O])->Result<usize,Error>{
 if prefix.is_empty()||prefix.len()>48||!prefix.iter().all(|&b|word(b)){
  return Err(Error::Invalid);
 }
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut bytes=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut bytes[..len]).map_err(|_|Error::Missing)?;
 let mut stage=[EMPTY;O];let mut count=0;let mut cursor=0;
 while cursor<len{
  if !word(bytes[cursor]){cursor+=1;continue;}
  let start=cursor;
  while cursor<len&&word(bytes[cursor]){cursor+=1;}
  let tok=&bytes[start..cursor];
  if tok.len()<=prefix.len()||tok.len()>48||!tok.starts_with(prefix){continue;}
  if let Some(pos)=stage[..count].iter().position(|e|e.len==tok.len()&&
   &e.bytes[..e.len]==tok){
   stage[pos].count=stage[pos].count.saturating_add(1);
  }else{
   if count==O{return Err(Error::Full);}
   stage[count].bytes[..tok.len()].copy_from_slice(tok);
   stage[count].len=tok.len();stage[count].count=1;count+=1;
  }
 }
 stage[..count].sort_unstable_by(|a,b|b.count.cmp(&a.count)
  .then(a.bytes[..a.len].cmp(&b.bytes[..b.len])));
 *out=stage;Ok(count)
}
pub fn most_common(candidates:&[Candidate])->Option<Candidate>{candidates.first().copied()}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn completion_returns_deduped_function_names_from_real_file(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"hello helloWorld helloWorld helloThere").unwrap();
  let mut out=[EMPTY;3];
  assert_eq!(index(&fs,b"/a",b"hello",&mut out),Ok(2));
  assert_eq!(&out[0].bytes[..out[0].len],b"helloWorld");
  assert_eq!(out[0].count,2);
 }
 #[test]fn insufficient_suggestion_slots_do_not_leak_partial_results(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"alpha alphabet").unwrap();
  let mut out=[Candidate{count:77,..EMPTY};1];
  assert_eq!(index(&fs,b"/a",b"al",&mut out),Err(Error::Full));
  assert_eq!(out[0].count,77);
 }
}