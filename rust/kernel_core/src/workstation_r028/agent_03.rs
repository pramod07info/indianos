//! R028/03: inspect actual file bytes for text format, UTF-8, BOM and line endings.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Type{Utf8,Utf8Bom,Binary}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Eol{None,Lf,CrLf,Mixed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Info{pub kind:Type,pub eol:Eol,pub lines:usize,
 pub bytes:usize,pub revision:u64}
pub fn inspect<const F:usize>(fs:&RamFs<F>,path:&[u8])->Result<Info,Error>{
 let(revision,bytes)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..bytes]).map_err(|_|Error::Missing)?;
 let text=&data[..bytes];
 let kind=if text.iter().any(|b|*b==0){Type::Binary}
  else if text.starts_with(&[0xef,0xbb,0xbf])&&
   core::str::from_utf8(&text[3..]).is_ok(){Type::Utf8Bom}
  else if core::str::from_utf8(text).is_ok(){Type::Utf8}else{Type::Binary};
 let mut lf=0;let mut crlf=0;
 for i in 0..bytes{if text[i]==b'\n'{lf+=1;if i>0&&text[i-1]==b'\r'{crlf+=1;}}}
 let eol=if lf==0{Eol::None}else if crlf==0{Eol::Lf}
  else if crlf==lf{Eol::CrLf}else{Eol::Mixed};
 let lines=if bytes==0{0}else{lf+usize::from(text[bytes-1]!=b'\n')};
 Ok(Info{kind,eol,lines,bytes,revision})
}
pub fn safe_edit(info:Info)->Result<(),Error>{
 if info.kind==Type::Binary{return Err(Error::Invalid);}Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn actual_file_crlf_and_utf8_bom_reported_correctly(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",&[0xef,0xbb,0xbf,b'a',b'\r',b'\n',b'b',b'\r',b'\n']).unwrap();
  let info=inspect(&fs,b"/a").unwrap();
  assert_eq!(info.kind,Type::Utf8Bom);assert_eq!(info.eol,Eol::CrLf);
  assert_eq!(info.lines,2);
 }
 #[test]fn binary_documents_are_rejected_by_source_editor(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"\x00\xff").unwrap();
  assert_eq!(safe_edit(inspect(&fs,b"/a").unwrap()),Err(Error::Invalid));
 }
}