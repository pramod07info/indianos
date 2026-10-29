//! R029/08: toggle Rust source line comment at UTF-8 safe line boundaries.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn toggle<const F:usize>(fs:&mut RamFs<F>,path:&[u8],revision:u64,
 line:usize)->Result<u64,Error>{
 if line==0{return Err(Error::Invalid);}
 let (rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..n]).map_err(|_|Error::Missing)?;
 let text=core::str::from_utf8(&data[..n]).map_err(|_|Error::Invalid)?;
 let mut begin=0;for _ in 1..line{
  begin=data[begin..n].iter().position(|&b|b==b'\n').map(|x|begin+x+1)
   .ok_or(Error::Missing)?;
 }
 if begin==n{return Err(Error::Missing);}
 let end=data[begin..n].iter().position(|&b|b==b'\n').map(|x|begin+x).unwrap_or(n);
 let mut cursor=begin;while cursor<end&&matches!(data[cursor],b' '|b'\t'){cursor+=1;}
 if !text.is_char_boundary(cursor){return Err(Error::Invalid);}
 let active=data[cursor..end].starts_with(b"//");
 let newlen=if active{n-2}else{n.checked_add(2).ok_or(Error::Overflow)?};
 if newlen>CONTENT_MAX{return Err(Error::Full);}
 let mut output=[0u8;CONTENT_MAX];
 output[..cursor].copy_from_slice(&data[..cursor]);
 let split=if active{cursor+2}else{cursor};
 let shift=if active{0}else{2};
 if !active{output[cursor..cursor+2].copy_from_slice(b"//");}
 output[cursor+shift..newlen].copy_from_slice(&data[split..n]);
 fs.replace(path,&output[..newlen]).map_err(|_|Error::Full)
}
pub fn is_commented(line:&[u8])->bool{
 let start=line.iter().take_while(|&&b|matches!(b,b' '|b'\t')).count();
 line[start..].starts_with(b"//")
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn toggle_real_second_line_comment_and_uncomment(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"fn a()\n  work();\n").unwrap();
  toggle(&mut fs,b"/a",1,2).unwrap();
  let mut out=[0;30];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"fn a()\n  //work();\n");
  toggle(&mut fs,b"/a",2,2).unwrap();
  let n=fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out[..n],b"fn a()\n  work();\n");
 }
 #[test]fn stale_version_refuses_comment_operation(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"a").unwrap();
  assert_eq!(toggle(&mut fs,b"/a",0,1),Err(Error::Stale));
 }
}