//! R030/12: real terminal tail with complete line boundaries.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn tail<const F:usize>(fs:&RamFs<F>,path:&[u8],count:usize,
 out:&mut[u8])->Result<usize,Error>{
 if count==0{return Err(Error::Invalid);}
 let (_,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..n])
  .map_err(|_|Error::Missing)?;
 let mut first=0;let mut found=0;
 for i in (0..n).rev(){
  if bytes[i]==b'\n'&&i+1<n{
   found+=1;if found==count{first=i+1;break;}
  }
 }
 let size=n-first;if size>out.len(){return Err(Error::Full);}
 out[..size].copy_from_slice(&bytes[first..n]);Ok(size)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn tail_reads_last_two_complete_real_log_lines(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/log",b"one\ntwo\nthree\n").unwrap();
  let mut out=[0;16];let n=tail(&fs,b"/p/log",2,&mut out).unwrap();
  assert_eq!(&out[..n],b"two\nthree\n");
 }
 #[test]fn tail_fails_without_partial_command_output(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/p/log",b"hello").unwrap();
  assert_eq!(tail(&fs,b"/p/log",1,&mut[0;2]),Err(Error::Full));
  assert_eq!(tail(&fs,b"/p/log",0,&mut[0;9]),Err(Error::Invalid));
 }
}