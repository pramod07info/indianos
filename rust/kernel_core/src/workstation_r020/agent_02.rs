//! R020/02 source editor line/column position, indentation and selections.
//! Byte-oriented source editing: no false claim of full Unicode grapheme support.
use super::Error;
use crate::workstation_r018::editor::Buffer;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Cursor{pub line:usize,pub column:usize,pub byte:usize}
pub fn position<const B:usize>(doc:&Buffer<B>,owner:u32,
 byte:usize)->Result<Cursor,Error>{
 if byte>doc.len(){return Err(Error::Invalid);}
 let mut text=[0u8;B];doc.copy(owner,0,doc.len(),&mut text)
  .map_err(|_|Error::Denied)?;
 let mut line=1;let mut column=1;
 for &b in &text[..byte]{if b==b'\n'{line+=1;column=1;}else{column+=1;}}
 Ok(Cursor{line,column,byte})
}
pub fn locate<const B:usize>(doc:&Buffer<B>,owner:u32,
 line:usize,column:usize)->Result<Cursor,Error>{
 if line==0||column==0{return Err(Error::Invalid);}
 let mut text=[0u8;B];doc.copy(owner,0,doc.len(),&mut text)
  .map_err(|_|Error::Denied)?;
 let mut current=1;let mut col=1;
 for i in 0..=doc.len(){
  if current==line&&col==column{return Ok(Cursor{line,column,byte:i});}
  if i==doc.len(){break;}
  if text[i]==b'\n'{current+=1;col=1;}else{col+=1;}
 }
 Err(Error::Missing)
}
pub fn indent<const B:usize>(doc:&mut Buffer<B>,owner:u32,revision:u64,
 line:usize,spaces:usize)->Result<u64,Error>{
 if spaces>16||spaces==0{return Err(Error::Invalid);}
 let at=locate(doc,owner,line,1)?.byte;
 doc.insert(owner,revision,at,&[b' ';16][..spaces])
  .map_err(|_|Error::Full)
}
pub fn delete_line<const B:usize>(doc:&mut Buffer<B>,owner:u32,
 revision:u64,line:usize)->Result<u64,Error>{
 let start=locate(doc,owner,line,1)?.byte;
 let mut data=[0u8;B];doc.copy(owner,0,doc.len(),&mut data)
  .map_err(|_|Error::Denied)?;
 let mut end=start;
 while end<doc.len()&&data[end]!=b'\n'{end+=1;}
 if end<doc.len(){end+=1;}
 doc.delete(owner,revision,start,end).map_err(|_|Error::Full)
}
pub fn line_count<const B:usize>(doc:&Buffer<B>,owner:u32)->Result<usize,Error>{
 let mut data=[0u8;B];doc.copy(owner,0,doc.len(),&mut data).map_err(|_|Error::Denied)?;
 Ok(1+data[..doc.len()].iter().filter(|&&b|b==b'\n').count())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn maps_editor_selection_and_creates_indent(){
  let mut e=Buffer::<80>::new(7);e.load(7,0,b"fn main\nreturn 1").unwrap();
  assert_eq!(position(&e,7,8).unwrap().line,2);
  assert_eq!(locate(&e,7,2,1).unwrap().byte,8);
  indent(&mut e,7,1,2,4).unwrap();
  assert_eq!(position(&e,7,12).unwrap().column,5);
  assert_eq!(line_count(&e,7),Ok(2));
 }
 #[test]fn line_deletion_does_not_affect_neighbor_source(){
  let mut e=Buffer::<80>::new(7);e.load(7,0,b"one\ntwo\nthree").unwrap();
  delete_line(&mut e,7,1,2).unwrap();
  let mut got=[0u8;30];let n=e.copy(7,0,e.len(),&mut got).unwrap();
  assert_eq!(&got[..n],b"one\nthree");
 }
 #[test]fn unauthorized_or_offscreen_position_is_rejected(){
  let mut e=Buffer::<20>::new(7);e.load(7,0,b"hello").unwrap();
  assert_eq!(locate(&e,7,99,1),Err(Error::Missing));
  assert_eq!(line_count(&e,8),Err(Error::Denied));
  assert_eq!(indent(&mut e,7,1,1,17),Err(Error::Invalid));
 }
}
