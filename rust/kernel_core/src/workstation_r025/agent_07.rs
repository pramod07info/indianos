//! R025/07 editor code actions: trim, indent and unindent actual source lines.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action{TrimTrailing,Indent,Unindent}
pub fn transform<const B:usize>(source:&[u8],line:usize,
 action:Action,out:&mut[u8;B])->Result<usize,Error>{
 if line==0{return Err(Error::Invalid);}
 let mut current=1usize;let mut start=0usize;
 for (i,&b) in source.iter().enumerate(){
  if current==line{break;}
  if b==b'\n'{current+=1;start=i+1;}
 }
 if current!=line||start>source.len(){return Err(Error::Missing);}
 let end=source[start..].iter().position(|b|*b==b'\n')
  .map(|i|start+i).unwrap_or(source.len());
 let old=&source[start..end];
 let pad=if action==Action::Indent{4}else{0};
 let drop_left=if action==Action::Unindent{
  old.iter().take_while(|&&b|b==b' ').count().min(4)
 }else{0};
 let drop_right=if action==Action::TrimTrailing{
  old.iter().rev().take_while(|&&b|b==b' '||b==b'\t').count()
 }else{0};
 let updated=&old[drop_left..old.len()-drop_right];
 let total=source.len()-old.len()+updated.len()+pad;
 if total>B{return Err(Error::Full);}
 let mut stage=[0u8;B];
 stage[..start].copy_from_slice(&source[..start]);
 for b in &mut stage[start..start+pad]{*b=b' ';}
 stage[start+pad..start+pad+updated.len()].copy_from_slice(updated);
 stage[start+pad+updated.len()..total].copy_from_slice(&source[end..]);
 *out=stage;Ok(total)
}
pub fn apply<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected:u64,line:usize,action:Action)->Result<u64,Error>{
 let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if revision!=expected{return Err(Error::Stale);}
 let mut bytes=[0u8;CONTENT_MAX];fs.read(path,0,&mut bytes[..size])
  .map_err(|_|Error::Missing)?;
 let mut out=[0u8;CONTENT_MAX];let n=transform(&bytes[..size],line,action,&mut out)?;
 if out[..n]==bytes[..size]{return Ok(revision);}
 fs.replace(path,&out[..n]).map_err(|_|Error::Full)
}
pub fn preview<const F:usize>(fs:&RamFs<F>,path:&[u8],line:usize,
 action:Action,out:&mut[u8])->Result<usize,Error>{
 let (_,len)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..len])
  .map_err(|_|Error::Missing)?;
 let mut temp=[0u8;CONTENT_MAX];let n=transform(&src[..len],line,action,&mut temp)?;
 if out.len()<n{return Err(Error::Full);}
 out[..n].copy_from_slice(&temp[..n]);Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn real_editor_file_trims_trailing_spaces_and_indents(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one\nlet x;  \n").unwrap();
  apply(&mut fs,b"/a",1,2,Action::TrimTrailing).unwrap();
  apply(&mut fs,b"/a",2,2,Action::Indent).unwrap();
  let mut out=[0u8;32];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"one\n    let x;\n");
 }
 #[test]fn external_file_revision_change_blocks_editor_action(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"one").unwrap();
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(apply(&mut fs,b"/a",1,1,Action::Indent),Err(Error::Stale));
 }
 #[test]fn insufficient_preview_buffer_preserves_result_destination(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"line").unwrap();
  let mut out=[55;2];
  assert_eq!(preview(&fs,b"/a",1,Action::Indent,&mut out),Err(Error::Full));
  assert_eq!(out,[55;2]);
 }
}
