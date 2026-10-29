//! R028/11: bounded terminal output search with pagination.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hit{pub offset:usize,pub line:usize}
pub const EMPTY:Hit=Hit{offset:0,line:0};
pub fn search<const N:usize>(bytes:&[u8],needle:&[u8],
 start:usize,out:&mut[Hit;N])->Result<(usize,bool),Error>{
 if needle.is_empty()||start>bytes.len(){return Err(Error::Invalid);}
 let mut hits=[EMPTY;N];let mut count=0;let mut more=false;let mut line=1;
 for i in 0..bytes.len(){
  if i>=start&&i+needle.len()<=bytes.len()&&
   &bytes[i..i+needle.len()]==needle{
   if count==N{more=true;break;}
   hits[count]=Hit{offset:i,line};count+=1;
  }
  if bytes[i]==b'\n'{line+=1;}
 }
 *out=hits;Ok((count,more))
}
pub fn next_page<const N:usize>(hits:&[Hit;N],count:usize)
 ->Result<usize,Error>{
 if count==0||count>N{return Err(Error::Missing);}
 hits[count-1].offset.checked_add(1).ok_or(Error::Overflow)
}
pub fn count_lines(bytes:&[u8])->usize{
 if bytes.is_empty(){0}else{
  bytes.iter().filter(|&&x|x==b'\n').count()+usize::from(bytes.last()!=Some(&b'\n'))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn search_paginates_reported_terminal_output_without_data_loss(){
  let data=b"foo\nbar\nfoo\n";let mut out=[EMPTY;1];
  let(n,more)=search(data,b"foo",0,&mut out).unwrap();
  assert_eq!((n,more,out[0].line),(1,true,1));
  let next=next_page(&out,n).unwrap();
  let(n,more)=search(data,b"foo",next,&mut out).unwrap();
  assert_eq!((n,more,out[0].line),(1,false,3));
 }
 #[test]fn invalid_empty_query_is_rejected(){
  assert_eq!(search(b"foo",b"",0,&mut[EMPTY;1]),Err(Error::Invalid));
 }
}