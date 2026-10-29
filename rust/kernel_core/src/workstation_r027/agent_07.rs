//! R027/07: bounded lexical Rust brace fold ranges linked to real file revision.
//! Not a syntax parser; produces exact byte/line ranges for block-folding UI.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Fold{pub start:usize,pub end:usize,pub start_line:u32,pub end_line:u32,pub revision:u64}
pub const EMPTY:Fold=Fold{start:0,end:0,start_line:0,end_line:0,revision:0};
pub fn scan<const F:usize,const O:usize>(fs:&RamFs<F>,path:&[u8],
 out:&mut[Fold;O])->Result<usize,Error>{
 let (revision,size)=fs.metadata(path).map_err(|_|Error::Missing)?;
 let mut text=[0u8;CONTENT_MAX];
 fs.read(path,0,&mut text[..size]).map_err(|_|Error::Missing)?;
 let mut stack=[(0usize,0u32);32];let mut depth=0;
 let mut stage=[EMPTY;O];let mut n=0;let mut line=1u32;
 for (i,&ch) in text[..size].iter().enumerate(){
  if ch==b'{'{
   if depth==stack.len(){return Err(Error::Full);}
   stack[depth]=(i,line);depth+=1;
  }else if ch==b'}'{
   if depth==0{return Err(Error::Invalid);}
   depth-=1;let (start,first)=stack[depth];
   if first<line{
    if n==O{return Err(Error::Full);}
    stage[n]=Fold{start,end:i+1,start_line:first,end_line:line,revision};n+=1;
   }
  }
  if ch==b'\n'{line+=1;}
 }
 if depth!=0{return Err(Error::Invalid);}
 stage[..n].sort_unstable_by_key(|f|f.start);
 *out=stage;Ok(n)
}
pub fn valid<const F:usize>(fs:&RamFs<F>,path:&[u8],fold:Fold)->bool{
 fs.metadata(path).is_ok_and(|(rev,_)|rev==fold.revision)
}
pub fn hide_lines(folds:&[Fold])->usize{
 folds.iter().map(|f|(f.end_line-f.start_line) as usize).sum()
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn discovers_multiline_nested_code_blocks_in_source(){
  let mut fs=RamFs::<1>::new();
  fs.replace(b"/main.rs",b"fn x() {\n  work();\n}\n").unwrap();
  let mut out=[EMPTY;2];assert_eq!(scan(&fs,b"/main.rs",&mut out),Ok(1));
  assert_eq!((out[0].start_line,out[0].end_line),(1,3));
 }
 #[test]fn malformed_unbalanced_braces_are_not_published_as_folds(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"fn x() {\n").unwrap();
  let mut out=[EMPTY;1];assert_eq!(scan(&fs,b"/a.rs",&mut out),Err(Error::Invalid));
 }
 #[test]fn fold_ranges_invalidated_after_real_document_edit(){
  let mut fs=RamFs::<1>::new();fs.replace(b"/a.rs",b"{\n}\n").unwrap();
  let mut out=[EMPTY;1];scan(&fs,b"/a.rs",&mut out).unwrap();
  fs.replace(b"/a.rs",b"changed").unwrap();
  assert!(!valid(&fs,b"/a.rs",out[0]));
 }
}
