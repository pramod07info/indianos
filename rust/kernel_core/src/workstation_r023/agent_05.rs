//! R023/05: editor format-on-save primitives for line-endings and indentation.
//! Deterministic bounded byte transformations, not a semantic formatter.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Stats{pub lines:usize,pub trimmed:usize,pub tab_expanded:usize,
 pub crlf_converted:usize}
pub fn normalize<const B:usize>(input:&[u8],tab_width:usize,
 out:&mut[u8;B])->Result<(usize,Stats),Error>{
 if tab_width==0||tab_width>8{return Err(Error::Invalid);}
 let mut temp=[0u8;B];let mut p=0;let mut line_start=0usize;
 let mut stats=Stats{lines:1,trimmed:0,tab_expanded:0,crlf_converted:0};
 let mut i=0;
 while i<input.len(){
  let b=input[i];
  if b==b' '&&(i+1==input.len()||input[i+1]==b'\n'||input[i+1]==b'\r'){
   stats.trimmed+=1;i+=1;continue;
  }
  if b==b'\t'{
   let spaces=tab_width-(p-line_start)%tab_width;
   if p+spaces>B{return Err(Error::Full);}
   for dst in &mut temp[p..p+spaces]{*dst=b' ';}
   p+=spaces;stats.tab_expanded+=1;i+=1;continue;
  }
  if b==b'\r'&&input.get(i+1)==Some(&b'\n'){
   stats.crlf_converted+=1;i+=1;continue;
  }
  if p>=B{return Err(Error::Full);}
  temp[p]=b;p+=1;i+=1;
  if b==b'\n'{stats.lines+=1;line_start=p;}
 }
 *out=temp;Ok((p,stats))
}
pub fn format_file<const F:usize>(fs:&mut RamFs<F>,
 path:&[u8],expected_version:u64,tab_width:usize)->Result<Stats,Error>{
 let (version,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 if version!=expected_version{return Err(Error::Stale);}
 let mut source=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut source[..size]).map_err(|_|Error::Missing)?;
 let mut formatted=[0u8;CONTENT_MAX];
 let (n,stats)=normalize(&source[..size],tab_width,&mut formatted)?;
 if source[..size]!=formatted[..n]{
  fs.replace(path,&formatted[..n]).map_err(|_|Error::Full)?;
 }
 Ok(stats)
}
pub fn count_trailing_spaces(data:&[u8])->usize{
 data.split(|b|*b==b'\n').map(|l|l.iter().rev().take_while(|&&b|b==b' ').count()).sum()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn formats_real_file_converting_crlf_and_tab_indentation(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/a",b"\tlet a = 1;  \r\n").unwrap();
  let stats=format_file(&mut fs,b"/a",1,4).unwrap();
  assert_eq!(stats.tab_expanded,1);assert_eq!(stats.crlf_converted,1);
  let mut out=[0u8;32];let n=fs.read(b"/a",0,&mut out).unwrap();
  assert_eq!(&out[..n],b"    let a = 1;\n");
 }
 #[test]fn output_capacity_failure_leaves_original_text_intact(){
  let mut out=[99u8;4];
  assert_eq!(normalize(b"\t",8,&mut out),Err(Error::Full));
  assert_eq!(out,[99;4]);
 }
 #[test]fn stale_source_revision_does_not_format_other_users_changes(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a",b" \n").unwrap();
  assert_eq!(format_file(&mut fs,b"/a",0,4),Err(Error::Stale));
  assert_eq!(count_trailing_spaces(b"x  \ny \n"),3);
 }
}
