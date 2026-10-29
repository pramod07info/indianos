//! R030/08: move a real UTF-8 source line up or down without losing newline content.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Direction{Up,Down}
fn ranges<const N:usize>(src:&[u8],out:&mut[(usize,usize);N])->Result<usize,Error>{
 let mut n=0;let mut start=0;
 for(i,&b) in src.iter().enumerate(){
  if b==b'\n'{
   if n==N{return Err(Error::Full);}
   out[n]=(start,i+1);n+=1;start=i+1;
  }
 }
 if start<src.len(){
  if n==N{return Err(Error::Full);}
  out[n]=(start,src.len());n+=1;
 }Ok(n)
}
pub fn move_line<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 revision:u64,line:usize,direction:Direction)->Result<u64,Error>{
 let (rev,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if revision!=rev{return Err(Error::Stale);}
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..len]).map_err(|_|Error::Missing)?;
 core::str::from_utf8(&src[..len]).map_err(|_|Error::Invalid)?;
 let mut spans=[(0usize,0usize);256];let count=ranges(&src[..len],&mut spans)?;
 if line==0||line>count{return Err(Error::Missing);}
 let a=line-1;let b=match direction{
  Direction::Up=>a.checked_sub(1).ok_or(Error::Invalid)?,
  Direction::Down=>if a+1<count{a+1}else{return Err(Error::Invalid)}
 };
 let first=a.min(b);let second=a.max(b);
 let (sa,ea)=spans[first];let(sb,eb)=spans[second];
 if ea!=sb{return Err(Error::Invalid);}
 let mut output=[0u8;CONTENT_MAX];
 output[..sa].copy_from_slice(&src[..sa]);
 let len_second=eb-sb;let len_first=ea-sa;
 output[sa..sa+len_second].copy_from_slice(&src[sb..eb]);
 output[sa+len_second..sa+len_second+len_first].copy_from_slice(&src[sa..ea]);
 output[eb..len].copy_from_slice(&src[eb..len]);
 fs.replace(path,&output[..len]).map_err(|_|Error::Full)
}
pub fn line_count(text:&[u8])->usize{
 text.iter().filter(|&&b|b==b'\n').count()+
 usize::from(!text.is_empty()&&text.last()!=Some(&b'\n'))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn moving_second_code_line_up_changes_real_document_order(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one\ntwo\nthree\n").unwrap();
  move_line(&mut fs,b"/a",1,2,Direction::Up).unwrap();
  let mut data=[0;14];fs.read(b"/a",0,&mut data).unwrap();
  assert_eq!(&data,b"two\none\nthree\n");
 }
 #[test]fn moving_first_line_up_fails_without_source_change(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a\nb\n").unwrap();
  assert_eq!(move_line(&mut fs,b"/a",1,1,Direction::Up),Err(Error::Invalid));
  assert_eq!(fs.version(),1);
 }
}