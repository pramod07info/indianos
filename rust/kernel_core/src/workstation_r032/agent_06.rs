//! R032/06: bounded line-by-line review between actual guest source files.
use super::Error;
use crate::batch7_ramfs::{RamFs,CONTENT_MAX};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Added,Removed,Changed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Hunk{pub line:usize,pub old_bytes:usize,pub new_bytes:usize,pub kind:Kind}
pub const EMPTY:Hunk=Hunk{line:0,old_bytes:0,new_bytes:0,kind:Kind::Changed};
fn next(data:&[u8],offset:usize)->Option<(usize,usize)>{
 if offset>=data.len(){return None;}
 let mut end=offset;while end<data.len()&&data[end]!=b'\n'{end+=1;}
 if end<data.len(){end+=1;}Some((end, end-offset))
}
pub fn compare<const F:usize,const N:usize>(fs:&RamFs<F>,before:&[u8],
 after:&[u8],out:&mut[Hunk;N])->Result<usize,Error>{
 let (_,a_size)=fs.metadata(before).map_err(|_|Error::Missing)?;
 let (_,b_size)=fs.metadata(after).map_err(|_|Error::Missing)?;
 let mut a=[0u8;CONTENT_MAX];let mut b=[0u8;CONTENT_MAX];
 fs.read(before,0,&mut a[..a_size]).map_err(|_|Error::Missing)?;
 fs.read(after,0,&mut b[..b_size]).map_err(|_|Error::Missing)?;
 let(mut at,mut bt,mut line,mut n)=(0usize,0usize,1usize,0usize);
 let mut staged=[EMPTY;N];
 while at<a_size||bt<b_size{
  let aa=next(&a[..a_size],at);let bb=next(&b[..b_size],bt);
  let ae=aa.map_or(at,|x|x.0);let be=bb.map_or(bt,|x|x.0);
  if a[at..ae]!=b[bt..be]{
   if n==N{return Err(Error::Full);}
   let kind=match(aa,bb){
    (None,Some(_))=>Kind::Added,(Some(_),None)=>Kind::Removed,_=>Kind::Changed
   };
   staged[n]=Hunk{line,old_bytes:ae-at,new_bytes:be-bt,kind};n+=1;
  }
  at=ae;bt=be;line+=1;
 }
 *out=staged;Ok(n)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn detects_add_and_changed_real_source_lines(){
  let mut fs=RamFs::<2>::new();
  fs.replace(b"/before",b"one\ntwo\n").unwrap();
  fs.replace(b"/after",b"one\nTWO\nextra\n").unwrap();
  let mut out=[EMPTY;3];assert_eq!(compare(&fs,b"/before",b"/after",&mut out),Ok(2));
  assert_eq!(out[0].kind,Kind::Changed);assert_eq!(out[1].kind,Kind::Added);
 }
 #[test]fn changed_lines_over_capacity_fail_without_partial_hunks(){
  let mut fs=RamFs::<2>::new();fs.replace(b"/a",b"a\nb").unwrap();
  fs.replace(b"/b",b"x\ny").unwrap();let mut out=[EMPTY;1];
  assert_eq!(compare(&fs,b"/a",b"/b",&mut out),Err(Error::Full));
  assert_eq!(out,[EMPTY]);
 }
}