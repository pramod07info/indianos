//! R030/04: UTF-8 correct byte-to-line/column cursor mapping.
use super::Error;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Cursor{pub line:usize,pub column:usize}
pub fn locate(source:&[u8],offset:usize)->Result<Cursor,Error>{
 let text=core::str::from_utf8(source).map_err(|_|Error::Invalid)?;
 if offset>source.len()||!text.is_char_boundary(offset){return Err(Error::Invalid);}
 let mut line=1;let mut column=1;
 for ch in text[..offset].chars(){
  if ch=='\n'{line+=1;column=1;}else{column+=1;}
 }
 Ok(Cursor{line,column})
}
pub fn offset(source:&[u8],cursor:Cursor)->Result<usize,Error>{
 if cursor.line==0||cursor.column==0{return Err(Error::Invalid);}
 let text=core::str::from_utf8(source).map_err(|_|Error::Invalid)?;
 let mut current=Cursor{line:1,column:1};
 if current==cursor{return Ok(0);}
 for (at,ch) in text.char_indices(){
  if ch=='\n'{current.line+=1;current.column=1;}
  else{current.column+=1;}
  if current==cursor{return Ok(at+ch.len_utf8());}
 }
 Err(Error::Missing)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn unicode_line_column_roundtrip_preserves_character_boundaries(){
  let s="αb\nc".as_bytes();
  assert_eq!(locate(s,2),Ok(Cursor{line:1,column:2}));
  assert_eq!(offset(s,Cursor{line:2,column:2}),Ok(5));
 }
 #[test]fn byte_offset_inside_multibyte_codepoint_is_rejected(){
  assert_eq!(locate("é".as_bytes(),1),Err(Error::Invalid));
  assert_eq!(offset(b"abc",Cursor{line:2,column:1}),Err(Error::Missing));
 }
}