//! R030/07: UTF-8 safe find/replace-all over real project file bytes, fail-atomic on size.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
pub fn transform(data:&[u8],needle:&[u8],replacement:&[u8],
 out:&mut[u8])->Result<(usize,usize),Error>{
 let original=core::str::from_utf8(data).map_err(|_|Error::Invalid)?;
 let target=core::str::from_utf8(needle).map_err(|_|Error::Invalid)?;
 let _=core::str::from_utf8(replacement).map_err(|_|Error::Invalid)?;
 if needle.is_empty()||target.len()>CONTENT_MAX{return Err(Error::Invalid);}
 let mut stage=[0u8;CONTENT_MAX];let mut pos=0;let mut cursor=0;let mut hits=0;
 while cursor<data.len(){
  if original.is_char_boundary(cursor)&&data[cursor..].starts_with(needle){
   if pos+replacement.len()>CONTENT_MAX{return Err(Error::Full);}
   stage[pos..pos+replacement.len()].copy_from_slice(replacement);
   pos+=replacement.len();cursor+=needle.len();hits+=1;
  }else{
   let next=original[cursor..].chars().next().ok_or(Error::Invalid)?.len_utf8();
   if pos+next>CONTENT_MAX{return Err(Error::Full);}
   stage[pos..pos+next].copy_from_slice(&data[cursor..cursor+next]);
   pos+=next;cursor+=next;
  }
 }
 if pos>out.len(){return Err(Error::Full);}
 out[..pos].copy_from_slice(&stage[..pos]);Ok((pos,hits))
}
pub fn replace<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 expected:u64,needle:&[u8],replacement:&[u8])->Result<usize,Error>{
 let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if revision!=expected{return Err(Error::Stale);}
 let mut src=[0u8;CONTENT_MAX];fs.read(path,0,&mut src[..size]).map_err(|_|Error::Missing)?;
 let mut dst=[0u8;CONTENT_MAX];let(n,count)=transform(&src[..size],needle,replacement,&mut dst)?;
 if count!=0{fs.replace(path,&dst[..n]).map_err(|_|Error::Full)?;}
 Ok(count)
}
pub fn matches(data:&[u8],needle:&[u8])->usize{
 if needle.is_empty(){return 0;}
 data.windows(needle.len()).filter(|x|*x==needle).count()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn unicode_safe_replacement_changes_real_editor_file(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a","é é and é".as_bytes()).unwrap();
  assert_eq!(replace(&mut fs,b"/a",1,"é".as_bytes(),b"ok"),Ok(3));
  let mut out=[0;16];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"ok ok and ok");
 }
 #[test]fn oversized_replace_preserves_previous_source(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"aa").unwrap();
  assert_eq!(replace(&mut fs,b"/a",1,b"a",&[b'X';CONTENT_MAX]),Err(Error::Full));
  assert_eq!(fs.version(),1);
 }
}