//! R028/09: normalize whitespace and line endings in a real UTF-8 source file.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Report{pub trimmed:usize,pub crlf:usize,pub bytes:usize}
pub fn clean(data:&[u8],out:&mut[u8])->Result<Report,Error>{
 if core::str::from_utf8(data).is_err(){return Err(Error::Invalid);}
 let mut buffer=[0u8;CONTENT_MAX];let mut n=0;let mut trimmed=0;let mut crlf=0;
 for line in data.split_inclusive(|&b|b==b'\n'){
  let with_newline=line.last()==Some(&b'\n');
  let raw=if with_newline{&line[..line.len()-1]}else{line};
  let raw=if raw.last()==Some(&b'\r'){crlf+=1;&raw[..raw.len()-1]}else{raw};
  let end=raw.iter().rposition(|&b|b!=b' '&&b!=b'\t').map(|x|x+1).unwrap_or(0);
  trimmed+=raw.len()-end;
  let size=end+usize::from(with_newline);
  if n+size>CONTENT_MAX{return Err(Error::Full);}
  buffer[n..n+end].copy_from_slice(&raw[..end]);n+=end;
  if with_newline{buffer[n]=b'\n';n+=1;}
 }
 if n>out.len(){return Err(Error::Full);}
 out[..n].copy_from_slice(&buffer[..n]);
 Ok(Report{trimmed,crlf,bytes:n})
}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected:u64)->Result<Report,Error>{
 let(rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=expected{return Err(Error::Stale);}
 let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..size])
  .map_err(|_|Error::Missing)?;
 let mut out=[0u8;CONTENT_MAX];
 let report=clean(&bytes[..size],&mut out)?;
 if out[..report.bytes]!=bytes[..size]{
  fs.replace(path,&out[..report.bytes]).map_err(|_|Error::Full)?;
 }
 Ok(report)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn normalizes_real_mixed_newlines_and_trailing_whitespace(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"a \r\nb\t\n").unwrap();
  let report=apply(&mut fs,b"/a",1).unwrap();
  assert_eq!((report.crlf,report.trimmed),(1,2));
  let mut out=[0u8;4];fs.read(b"/a",0,&mut out).unwrap();assert_eq!(&out,b"a\nb\n");
 }
 #[test]fn invalid_utf8_data_rejected_without_file_replacement(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"\xff").unwrap();
  assert_eq!(apply(&mut fs,b"/a",1),Err(Error::Invalid));
  assert_eq!(fs.version(),1);
 }
}