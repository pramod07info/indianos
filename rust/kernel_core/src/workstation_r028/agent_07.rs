//! R028/07: character-aware line/column to byte-offset source navigation.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn offset(bytes:&[u8],line:usize,column:usize)->Result<usize,Error>{
 if line==0||column==0{return Err(Error::Invalid);}
 let text=core::str::from_utf8(bytes).map_err(|_|Error::Invalid)?;
 let mut start=0usize;let mut row=1;
 while row<line{
  let next=bytes[start..].iter().position(|b|*b==b'\n').ok_or(Error::Missing)?;
  start+=next+1;row+=1;
 }
 if start>bytes.len(){return Err(Error::Missing);}
 let end=bytes[start..].iter().position(|b|*b==b'\n')
  .map(|n|start+n).unwrap_or(bytes.len());
 let fragment=text.get(start..end).ok_or(Error::Invalid)?;
 if column==fragment.chars().count()+1{return Ok(end);}
 fragment.char_indices().nth(column-1).map(|(n,_)|start+n).ok_or(Error::Invalid)
}
pub fn locate<const F:usize>(fs:&RamFs<F>,path:&[u8],
 revision:u64,line:usize,column:usize)->Result<usize,Error>{
 let(rev,n)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..n]).map_err(|_|Error::Missing)?;
 offset(&source[..n],line,column)
}
pub fn count_lines(bytes:&[u8])->usize{
 if bytes.is_empty(){0}else{bytes.iter().filter(|&&x|x==b'\n').count()
  +usize::from(bytes.last()!=Some(&b'\n'))}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn utf8_character_column_is_not_raw_byte_position(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a","éx\nABC".as_bytes()).unwrap();
  assert_eq!(locate(&fs,b"/a",1,1,2),Ok(2));
  assert_eq!(locate(&fs,b"/a",1,2,2),Ok(5));
 }
 #[test]fn stale_document_and_nonexistent_column_are_rejected(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"abc").unwrap();
  assert_eq!(locate(&fs,b"/a",1,1,9),Err(Error::Invalid));
  fs.replace(b"/a",b"new").unwrap();
  assert_eq!(locate(&fs,b"/a",1,1,1),Err(Error::Stale));
 }
}