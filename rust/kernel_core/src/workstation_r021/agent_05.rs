//! R021/05 prefix completion over actual bounded RAM editor source.
//! No speculative network AI calls or arbitrary compiler execution.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Candidate{pub bytes:[u8;64],pub len:usize,pub occurrences:u16}
pub const EMPTY:Candidate=Candidate{bytes:[0;64],len:0,occurrences:0};
fn word(b:u8)->bool{b.is_ascii_alphanumeric()||b==b'_'}
pub fn complete<const N:usize>(source:&[u8],prefix:&[u8],
 out:&mut[Candidate;N])->Result<usize,Error>{
 if prefix.is_empty()||prefix.len()>64||
  !prefix.iter().all(|b|word(*b)){return Err(Error::Invalid);}
 let mut staged=[EMPTY;N];let mut count=0;let mut cursor=0;
 while cursor<source.len(){
  if !word(source[cursor]){cursor+=1;continue;}
  let begin=cursor;
  while cursor<source.len()&&word(source[cursor]){cursor+=1;}
  let token=&source[begin..cursor];
  if token.len()<=prefix.len()||token.len()>64||!token.starts_with(prefix){
   continue;
  }
  if let Some(i)=staged[..count].iter().position(|c|
    &c.bytes[..c.len]==token){
   staged[i].occurrences=staged[i].occurrences.saturating_add(1);
  }else{
   if count>=N{return Err(Error::Full);}
   let mut c=EMPTY;c.bytes[..token.len()].copy_from_slice(token);
   c.len=token.len();c.occurrences=1;staged[count]=c;count+=1;
  }
 }
 staged[..count].sort_unstable_by(|a,b|b.occurrences.cmp(&a.occurrences)
  .then_with(||a.bytes[..a.len].cmp(&b.bytes[..b.len])));
 *out=staged;Ok(count)
}
pub fn in_file<const F:usize,const O:usize>(fs:&RamFs<F>,path:&[u8],
 prefix:&[u8],out:&mut[Candidate;O])->Result<usize,Error>{
 let (_,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut data=[0u8;CONTENT_MAX];
 let n=fs.read(path,0,&mut data[..size]).map_err(|_|Error::Missing)?;
 complete(&data[..n],prefix,out)
}
pub fn best<'a>(results:&'a[Candidate])->Option<&'a[u8]>{
 results.first().map(|c|&c.bytes[..c.len])
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn autocomplete_ranks_duplicate_matching_identifiers(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/src/a",b"mainline mainline mainboard minor").unwrap();
  let mut out=[EMPTY;4];
  let n=in_file(&fs,b"/src/a",b"main",&mut out).unwrap();
  assert_eq!(n,2);assert_eq!(best(&out[..n]),Some(b"mainline".as_slice()));
  assert_eq!(out[0].occurrences,2);
 }
 #[test]fn tiny_result_buffer_does_not_publish_partial_completion(){
  let mut out=[Candidate{len:55,..EMPTY};1];
  assert_eq!(complete(b"mainline mainboard",b"main",&mut out),Err(Error::Full));
  assert_eq!(out[0].len,55);
 }
 #[test]fn empty_prefix_or_missing_file_is_denied(){
  let mut out=[EMPTY;2];
  assert_eq!(complete(b"hello",b"",&mut out),Err(Error::Invalid));
  assert_eq!(in_file(&RamFs::<1>::new(),b"/missing",b"a",&mut out),
   Err(Error::Missing));
 }
}
