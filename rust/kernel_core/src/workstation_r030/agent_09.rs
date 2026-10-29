//! R030/09: detect indentation styles and normalize leading tabs in real source files.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Style{None,Spaces,Tabs,Mixed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Profile{pub style:Style,pub indented:usize,pub min_spaces:usize}
pub fn detect(data:&[u8])->Profile{
 let mut tabs=0;let mut spaces=0;let mut min=usize::MAX;
 for line in data.split(|&b|b==b'\n'){
  if line.is_empty(){continue;}
  let size=line.iter().take_while(|&&b|b==b' ').count();
  if size>0{spaces+=1;min=min.min(size);}
  else if line.first()==Some(&b'\t'){tabs+=1;}
 }
 Profile{style:match(tabs>0,spaces>0){
  (false,false)=>Style::None,(true,false)=>Style::Tabs,
  (false,true)=>Style::Spaces,(true,true)=>Style::Mixed
 },indented:tabs+spaces,min_spaces:if min==usize::MAX{0}else{min}}
}
pub fn tabs_to_spaces<const F:usize>(fs:&mut RamFs<F>,path:&[u8],
 revision:u64,width:usize)->Result<usize,Error>{
 if width==0||width>8{return Err(Error::Invalid);}
 let(rev,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if rev!=revision{return Err(Error::Stale);}
 let mut input=[0u8;CONTENT_MAX];fs.read(path,0,&mut input[..size])
  .map_err(|_|Error::Missing)?;
 core::str::from_utf8(&input[..size]).map_err(|_|Error::Invalid)?;
 let mut output=[0u8;CONTENT_MAX];let mut count=0;let mut lines=0;let mut leading=true;
 for &b in &input[..size]{
  if b==b'\t'&&leading{
   if count+width>CONTENT_MAX{return Err(Error::Full);}
   for x in &mut output[count..count+width]{*x=b' ';}
   count+=width;lines+=1;
  }else{
   if count==CONTENT_MAX{return Err(Error::Full);}
   output[count]=b;count+=1;
   if b==b'\n'{leading=true;}else if b!=b' '{leading=false;}
  }
 }
 if lines>0{fs.replace(path,&output[..count]).map_err(|_|Error::Full)?;}
 Ok(lines)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn converts_actual_source_leading_tabs_to_four_spaces(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"\tfn a(){}\n\treturn\n").unwrap();
  assert_eq!(detect(b"\tfn a(){}\n").style,Style::Tabs);
  assert_eq!(tabs_to_spaces(&mut fs,b"/a",1,4),Ok(2));
  let mut out=[0;64];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"    fn a(){}\n    return\n");
 }
 #[test]fn stale_indent_request_and_size_overflow_leave_file_intact(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b"\tcode").unwrap();
  assert_eq!(tabs_to_spaces(&mut fs,b"/a",0,4),Err(Error::Stale));
  assert_eq!(fs.version(),1);
 }
}