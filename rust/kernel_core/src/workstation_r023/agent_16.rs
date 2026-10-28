//! R023/16: compiler diagnostic quick-fix application to actual source files.
//! Exact preimage, version and source location guards prevent stale edits.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX,PATH_MAX};
use crate::workstation_r019::agent_04::{Diagnostic,Severity};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Fix{pub path:[u8;PATH_MAX],pub len:usize,
 pub offset:usize,pub version:u64,pub old_len:usize,
 pub old:[u8;64],pub new_len:usize,pub new:[u8;64],
 pub error_line:u32}
pub fn propose<const F:usize>(fs:&RamFs<F>,diagnostic:&Diagnostic,
 old:&[u8],new:&[u8])->Result<Fix,Error>{
 if diagnostic.severity!=Severity::Error||old.is_empty()||
  old.len()>64||new.len()>64||diagnostic.path_len==0||
  diagnostic.path_len>PATH_MAX{return Err(Error::Invalid);}
 let path=&diagnostic.path[..diagnostic.path_len];
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut data=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut data[..size]).map_err(|_|Error::Missing)?;
 let mut line=1u32;let mut from=0;
 for (i,&b) in data[..size].iter().enumerate(){
  if line==diagnostic.line{from=i;break;}
  if b==b'\n'{line+=1;from=i+1;}
 }
 if line!=diagnostic.line{return Err(Error::Missing);}
 let line_end=data[from..size].iter().position(|b|*b==b'\n')
  .map(|pos|from+pos).unwrap_or(size);
 let offset=data[from..line_end].windows(old.len())
  .position(|w|w==old).map(|pos|from+pos).ok_or(Error::Missing)?;
 let mut fix=Fix{path:[0;PATH_MAX],len:path.len(),
  offset,version,old_len:old.len(),old:[0;64],
  new_len:new.len(),new:[0;64],error_line:diagnostic.line};
 fix.path[..path.len()].copy_from_slice(path);
 fix.old[..old.len()].copy_from_slice(old);
 fix.new[..new.len()].copy_from_slice(new);Ok(fix)
}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,fix:Fix)
 ->Result<u64,Error>{
 if fix.len==0||fix.len>PATH_MAX||fix.old_len==0||
  fix.old_len>64||fix.new_len>64{return Err(Error::Invalid);}
 let path=&fix.path[..fix.len];
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=fix.version{return Err(Error::Stale);}
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 let end=fix.offset.checked_add(fix.old_len).ok_or(Error::Overflow)?;
 if end>size||source[fix.offset..end]!=fix.old[..fix.old_len]{
  return Err(Error::Stale);
 }
 let next=size-fix.old_len+fix.new_len;
 if next>CONTENT_MAX{return Err(Error::Full);}
 let mut result=[0u8;CONTENT_MAX];
 result[..fix.offset].copy_from_slice(&source[..fix.offset]);
 result[fix.offset..fix.offset+fix.new_len].copy_from_slice(&fix.new[..fix.new_len]);
 result[fix.offset+fix.new_len..next].copy_from_slice(&source[end..size]);
 fs.replace(path,&result[..next]).map_err(|_|Error::Full)
}
pub fn preview<const F:usize>(fs:&RamFs<F>,fix:Fix)->Result<isize,Error>{
 if fix.len==0||fix.len>PATH_MAX{return Err(Error::Invalid);}
 let (version,_) = fs.metadata(&fix.path[..fix.len]).map_err(|_|Error::Missing)?;
 if version!=fix.version{return Err(Error::Stale);}
 Ok(fix.new_len as isize-fix.old_len as isize)
}
#[cfg(test)]mod tests{
 use super::*;
 use crate::workstation_r019::agent_04::parse;
 #[test]fn compiler_diagnostic_replacement_updates_actual_source_line(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/main.rs",b"fn main(){\n  retrun 1;\n}\n").unwrap();
  let d=parse(b"/main.rs:2:3:error unknown symbol").unwrap();
  let fix=propose(&fs,&d,b"retrun",b"return").unwrap();
  assert_eq!(preview(&fs,fix),Ok(0));apply(&mut fs,fix).unwrap();
  let mut out=[0u8;40];let n=fs.read(b"/main.rs",0,&mut out).unwrap();
  assert!(out[..n].windows(6).any(|x|x==b"return"));
 }
 #[test]fn external_edit_cannot_be_overwritten_by_stale_quickfix(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"wrong").unwrap();
  let d=parse(b"/a:1:1:error wrong").unwrap();
  let fix=propose(&fs,&d,b"wrong",b"right").unwrap();
  fs.replace(b"/a",b"user edit").unwrap();
  assert_eq!(apply(&mut fs,fix),Err(Error::Stale));
 }
 #[test]fn quickfix_must_match_diagnostic_source_line(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one\ntwo").unwrap();
  let d=parse(b"/a:2:1:error wrong").unwrap();
  assert_eq!(propose(&fs,&d,b"one",b"new"),Err(Error::Missing));
 }
}
