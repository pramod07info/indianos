//! R029/12: bounded output paging and line-aware navigation on terminal bytes.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Page{pub offset:usize,pub len:usize,pub first_line:usize,pub has_more:bool}
pub fn page(bytes:&[u8],start:usize,max_lines:usize,
 out:&mut[u8])->Result<Page,Error>{
 if start>bytes.len()||max_lines==0{return Err(Error::Invalid);}
 if start>0&&bytes[start-1]!=b'\n'{return Err(Error::Invalid);}
 let mut end=start;let mut lines=0;
 while end<bytes.len()&&lines<max_lines{
  if bytes[end]==b'\n'{lines+=1;}
  end+=1;
 }
 if lines<max_lines&&end==bytes.len()&&end>start&&bytes[end-1]!=b'\n'{lines+=1;}
 let len=end-start;
 if len>out.len(){return Err(Error::Full);}
 out[..len].copy_from_slice(&bytes[start..end]);
 let first_line=1+bytes[..start].iter().filter(|&&b|b==b'\n').count();
 Ok(Page{offset:start,len,first_line,has_more:end<bytes.len()})
}
pub fn next(page:Page)->usize{page.offset+page.len}
pub fn total_lines(bytes:&[u8])->usize{
 bytes.iter().filter(|&&b|b==b'\n').count()
 +usize::from(!bytes.is_empty()&&bytes.last()!=Some(&b'\n'))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn large_terminal_output_paged_without_truncating_lines(){
  let bytes=b"one\ntwo\nthree\n";let mut out=[0;16];
  let a=page(bytes,0,2,&mut out).unwrap();
  assert_eq!(&out[..a.len],b"one\ntwo\n");
  let b=page(bytes,next(a),2,&mut out).unwrap();
  assert_eq!(&out[..b.len],b"three\n");
 }
 #[test]fn output_buffer_too_short_never_receives_partial_line(){
  let mut out=[88;2];
  assert_eq!(page(b"long\n",0,1,&mut out),Err(Error::Full));
  assert_eq!(out,[88;2]);
 }
}